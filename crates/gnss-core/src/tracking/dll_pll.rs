//! DLL + PLL tracking for GPS L1 C/A.
//!
//! Based on GNSS-SDR `dll_pll_veml_tracking.cc` + `Tracking_2nd_PLL_filter`.

use super::discriminators;
use super::loop_filter::LoopFilter;
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

#[derive(Debug, Clone)]
pub struct TrackingConfig {
    pub fs: f64, pub if_freq: f64,
    pub dll_bw: f64, pub pll_bw: f64, pub damping: f64, pub early_late_spc: f64,
}
impl Default for TrackingConfig {
    fn default() -> Self {
        Self { fs: 4_000_000.0, if_freq: 0.0, dll_bw: 4.0, pll_bw: 25.0, damping: 0.7, early_late_spc: 0.5 }
    }
}

#[derive(Debug, Clone)]
pub struct TrackingOutput {
    pub prompt: Complex<f64>, pub early_mag: f64, pub late_mag: f64,
    pub code_phase_chips: f64, pub carrier_doppler_hz: f64, pub cn0_db_hz: f64,
}

pub struct DllPllTracking {
    config: TrackingConfig, code: &'static [i8; CODE_LENGTH], t: f64,
    carrier_freq_hz: f64, carrier_phase_rad: f64,
    code_phase_chips: f64, code_freq_chips_per_s: f64,
    dll_filter: LoopFilter, pll_filter: LoopFilter,
    // C/N₀: smoothed I² and Q²
    cn0_i2: f64, cn0_q2: f64, cn0_n: u32,
    // Symbol accumulation
    symbol_acc: Complex<f64>, symbol_cnt: u32,
    // Bit sync: histogram over 20 possible boundaries
    /// Ring buffer of recent 1ms prompt I values (40 = 2×20)
    bs_buf: [f64; 40], bs_idx: usize,
    bs_energy: [f64; 20], bs_count: u32,
    pub bit_boundary: Option<usize>,
    /// Index into 20ms cycle; when bs_ms reaches boundary we sample
    bs_ms: usize,
}

impl DllPllTracking {
    pub fn new(prn: usize, doppler_hz: f64, code_phase_chips: f64, config: TrackingConfig) -> Self {
        let t = 0.001;
        Self { config: config.clone(), code: &PRN_CODES[prn], t,
            carrier_freq_hz: config.if_freq + doppler_hz, carrier_phase_rad: 0.0,
            code_phase_chips, code_freq_chips_per_s: 1.023e6,
            dll_filter: LoopFilter::new(config.dll_bw, config.damping, 1.0, t),
            pll_filter: LoopFilter::new(config.pll_bw, config.damping, 0.25, t),
            cn0_i2: 0.0, cn0_q2: 0.0, cn0_n: 0,
            symbol_acc: Complex::new(0.0, 0.0), symbol_cnt: 0,
            bs_buf: [0.0; 40], bs_idx: 0, bs_energy: [0.0; 20], bs_count: 0,
            bit_boundary: None, bs_ms: 0,
        }
    }

    pub fn process(&mut self, samples: &[Complex<f64>]) -> TrackingOutput {
        let fs = self.config.fs;
        let spc_chips = self.config.early_late_spc;
        let spc_s = spc_chips * fs / self.code_freq_chips_per_s;
        let (early, prompt, late) = self.correlate(samples, spc_s);

        // Loop updates
        let dll_err = discriminators::dll_discriminator(early, late, spc_chips);
        let dll_corr = self.dll_filter.update(-dll_err);
        let pll_err_rad = discriminators::pll_discriminator(prompt);
        let pll_corr_rad_per_s2 = self.pll_filter.update(pll_err_rad);
        let pll_corr_hz_per_s = pll_corr_rad_per_s2 / TAU;
        self.code_freq_chips_per_s = 1.023e6 + dll_corr;
        self.code_phase_chips += self.code_freq_chips_per_s * self.t;
        self.code_phase_chips %= CODE_LENGTH as f64;
        self.carrier_freq_hz += pll_corr_hz_per_s * self.t;
        let doppler = self.carrier_freq_hz - self.config.if_freq;

        // C/N₀
        self.update_cn0(prompt);
        let cn0 = self.compute_cn0();

        // Bit synchronization
        self.update_bit_sync(prompt);

        // Symbol accumulation
        self.symbol_acc += prompt;
        self.symbol_cnt += 1;

        TrackingOutput {
            prompt, early_mag: early.norm(), late_mag: late.norm(),
            code_phase_chips: self.code_phase_chips, carrier_doppler_hz: doppler,
            cn0_db_hz: cn0.max(0.0),
        }
    }

    pub fn take_symbol(&mut self) -> Option<Complex<f64>> {
        if self.symbol_cnt >= 20 {
            let s = self.symbol_acc;
            self.symbol_acc = Complex::new(0.0, 0.0);
            self.symbol_cnt = 0;
            Some(s)
        } else { None }
    }

    pub fn code_freq(&self) -> f64 { self.code_freq_chips_per_s }
    pub fn dll_state(&self) -> f64 { self.dll_filter.state() }
    pub fn pll_state(&self) -> f64 { self.pll_filter.state() }

    // --- private ---

    fn correlate(&mut self, samples: &[Complex<f64>], spc_s: f64) -> (Complex<f64>, Complex<f64>, Complex<f64>) {
        let fs = self.config.fs;
        let spc_chips = spc_s / fs * self.code_freq_chips_per_s;
        let mut e = Complex::new(0.0, 0.0);
        let mut p = Complex::new(0.0, 0.0);
        let mut l = Complex::new(0.0, 0.0);
        for (i, sample) in samples.iter().enumerate() {
            let carrier = Complex::new((TAU * self.carrier_phase_rad).cos(), -(TAU * self.carrier_phase_rad).sin());
            let wiped = sample * carrier;
            let cp = (self.code_phase_chips + i as f64 * self.code_freq_chips_per_s / fs) % CODE_LENGTH as f64;
            let ci = cp as usize % CODE_LENGTH;
            let ei = ((cp - spc_chips + CODE_LENGTH as f64) as usize) % CODE_LENGTH;
            e += wiped * Complex::new(self.code[ei] as f64, 0.0);
            p += wiped * Complex::new(self.code[ci] as f64, 0.0);
            let li = ((cp + spc_chips) as usize) % CODE_LENGTH;
            l += wiped * Complex::new(self.code[li] as f64, 0.0);
            self.carrier_phase_rad += self.carrier_freq_hz / fs;
            self.carrier_phase_rad -= self.carrier_phase_rad.floor();
        }
        (e, p, l)
    }

    fn update_cn0(&mut self, prompt: Complex<f64>) {
        let i2 = prompt.re * prompt.re;
        let q2 = prompt.im * prompt.im;
        let a = 0.01;
        self.cn0_i2 = (1.0 - a) * self.cn0_i2 + a * i2;
        self.cn0_q2 = (1.0 - a) * self.cn0_q2 + a * q2;
        self.cn0_n += 1;
    }

    fn compute_cn0(&self) -> f64 {
        if self.cn0_n < 20 || self.cn0_q2 < 1e-30 { return 0.0; }
        let ratio = self.cn0_i2 / self.cn0_q2;
        // C/N₀ ≈ 10*log10(ratio - 1) + 27.0 (for T=1ms)
        if ratio > 1.1 { 10.0 * (ratio - 1.0).log10() + 27.0 } else { 0.0 }
    }

    fn update_bit_sync(&mut self, prompt: Complex<f64>) {
        if self.bit_boundary.is_some() { return; }
        self.bs_buf[self.bs_idx % 40] = prompt.re;
        self.bs_idx += 1;
        if self.bs_idx < 20 { return; }
        // For each possible boundary, compute bit energy = |sum of 20 consecutive I|
        let start = self.bs_idx - 20;
        for b in 0..20 {
            let b0 = (start + b) % 40;
            let b1 = (b0 + 19) % 40;
            let sum: f64 = if b1 >= b0 {
                self.bs_buf[b0..=b1].iter().sum()
            } else {
                let s1: f64 = self.bs_buf[b0..].iter().sum();
                let s2: f64 = self.bs_buf[..=b1].iter().sum();
                s1 + s2
            };
            self.bs_energy[b] += sum.abs();
        }
        self.bs_count += 1;
        if self.bs_count >= 40 {
            let mut best = 0;
            let mut best_val = 0.0f64;
            for (i, &e) in self.bs_energy.iter().enumerate() {
                if e > best_val { best_val = e; best = i; }
            }
            self.bit_boundary = Some(best);
            // Switch boundary going forward (don't reset — first symbol may be mixed)
        }
    }
}
