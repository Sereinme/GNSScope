//! DLL + PLL tracking for GPS L1 C/A.
//!
//! Based on GNSS-SDR `dll_pll_veml_tracking.cc`.
//! - E/P/L correlators with configurable spacing
//! - Normalized DLL discriminator: (E-L)/(E+L) in chips
//! - Costas PLL discriminator: atan2(Q,I)
//! - 2nd order loop filters for DLL and PLL
//! - C/N₀ estimation

use super::discriminators;
use super::loop_filter::LoopFilter;
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

#[derive(Debug, Clone)]
pub struct TrackingConfig {
    pub fs: f64,
    pub if_freq: f64,
    pub dll_bw: f64,
    pub pll_bw: f64,
    pub damping: f64,
    pub early_late_spc: f64,
}

impl Default for TrackingConfig {
    fn default() -> Self {
        Self {
            fs: 4_000_000.0, if_freq: 0.0,
            dll_bw: 2.0, pll_bw: 25.0,
            damping: 0.707, early_late_spc: 0.5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrackingOutput {
    pub prompt: Complex<f64>,
    pub early_mag: f64,
    pub late_mag: f64,
    pub code_phase_chips: f64,
    pub carrier_doppler_hz: f64,
    pub cn0_db_hz: f64,
    pub pll_locked: bool,
}

pub struct DllPllTracking {
    prn: usize,
    config: TrackingConfig,
    code: &'static [i8; CODE_LENGTH],

    // Carrier NCO
    carrier_freq_hz: f64,
    init_carrier_freq_hz: f64,  // clamped ±500Hz from this
    carrier_phase_rad: f64,

    // Code NCO
    code_phase_chips: f64,
    code_freq_chips_per_s: f64,

    // Loop filters (maintain state across 1ms calls)
    dll_filter: LoopFilter,
    pll_filter: LoopFilter,

    // C/N₀ estimation
    cn0_wbp: f64,
    cn0_nbp: f64,
    cn0_n: u32,

    // Symbol accumulation (20ms per GPS nav bit)
    symbol_acc: Complex<f64>,
    symbol_cnt: u32,
}

impl DllPllTracking {
    pub fn new(prn: usize, doppler_hz: f64, code_phase_chips: f64, config: TrackingConfig) -> Self {
        let t = 0.001;
        Self {
            prn,
            config: config.clone(),
            code: &PRN_CODES[prn],
            carrier_freq_hz: config.if_freq + doppler_hz,
            init_carrier_freq_hz: config.if_freq + doppler_hz,
            carrier_phase_rad: 0.0,
            code_phase_chips,
            code_freq_chips_per_s: 1.023e6,
            dll_filter: LoopFilter::new(config.dll_bw, config.damping, 1.0, t),
            pll_filter: LoopFilter::new(config.pll_bw, config.damping, 1.0, t),
            cn0_wbp: 0.0, cn0_nbp: 0.0, cn0_n: 0,
            symbol_acc: Complex::new(0.0, 0.0), symbol_cnt: 0,
        }
    }

    /// Process one 1ms block. Returns tracking output + updated state.
    pub fn process(&mut self, samples: &[Complex<f64>]) -> TrackingOutput {
        let n = samples.len();
        let fs = self.config.fs;
        let code_rate = self.code_freq_chips_per_s;
        let spc = self.config.early_late_spc;
        let spc_s = spc * fs / code_rate;

        // Correlate: E, P, L
        let (early, prompt, late) = self.correlate(samples, spc_s, fs, code_rate);

        // Discriminators
        let dll_err = discriminators::dll_discriminator(early, late, spc);
        let pll_err = discriminators::pll_discriminator(prompt);

        // Loop filters → rate corrections
        // DLL: output in chips/s. Negative sign: if early > late, slow down code rate
        let dll_corr = self.dll_filter.update(-dll_err);
        // PLL: output in rad/s, convert to Hz
        let pll_corr = self.pll_filter.update(pll_err);
        let pll_hz = pll_corr / std::f64::consts::TAU;

        // Update NCOs
        // Code rate: nominal + DLL correction (chips/s)
        self.code_freq_chips_per_s = 1.023e6 + dll_corr;
        // Advance code phase by one code period
        self.code_phase_chips += self.code_freq_chips_per_s * 0.001;
        self.code_phase_chips %= CODE_LENGTH as f64;

        // Carrier frequency: IF + old_doppler + PLL correction
        // PLL correction is in rad/s, convert to Hz: rad/s / (2π) = Hz
        self.carrier_freq_hz = (self.carrier_freq_hz + pll_hz)
            .clamp(self.init_carrier_freq_hz - 500.0, self.init_carrier_freq_hz + 500.0);
        let doppler = self.carrier_freq_hz - self.config.if_freq;

        // Symbol accumulation (20ms = 1 bit)
        self.symbol_acc += prompt;
        self.symbol_cnt += 1;
        if self.symbol_cnt >= 20 {
            self.symbol_cnt = 0;
        }

        // C/N₀
        self.update_cn0(prompt);
        let cn0 = if self.cn0_n > 20 {
            10.0 * (self.cn0_nbp / (self.cn0_wbp - self.cn0_nbp + 1e-30)).log10() + 30.0
        } else { 0.0 };

        TrackingOutput {
            prompt, early_mag: early.norm(), late_mag: late.norm(),
            code_phase_chips: self.code_phase_chips,
            carrier_doppler_hz: doppler,
            cn0_db_hz: cn0.max(0.0),
            pll_locked: cn0 > 25.0,
        }
    }

    pub fn take_symbol(&mut self) -> Option<Complex<f64>> {
        if self.symbol_cnt == 0 {
            let s = self.symbol_acc;
            self.symbol_acc = Complex::new(0.0, 0.0);
            Some(s)
        } else { None }
    }

    pub fn set_carrier_freq(&mut self, f: f64) { self.carrier_freq_hz = f; }
    pub fn code_freq(&self) -> f64 { self.code_freq_chips_per_s }
    pub fn dll_state(&self) -> f64 { self.dll_filter.state() }
    pub fn pll_state(&self) -> f64 { self.pll_filter.state() }

    // --- private ---

    fn correlate(&mut self, samples: &[Complex<f64>], spc_samples: f64, fs: f64, code_rate: f64) -> (Complex<f64>, Complex<f64>, Complex<f64>) {
        let n = samples.len();
        let spc = spc_samples / fs * code_rate; // spacing in chips
        let mut e = Complex::new(0.0, 0.0);
        let mut p = Complex::new(0.0, 0.0);
        let mut l = Complex::new(0.0, 0.0);

        for (i, sample) in samples.iter().enumerate() {
            // Carrier wipe-off (note: negative phase for down-conversion)
            let phase = self.carrier_phase_rad;
            let carrier = Complex::new((TAU * phase).cos(), -(TAU * phase).sin());
            let wiped = sample * carrier;

            // Code phase for this sample
            let cp = (self.code_phase_chips + i as f64 * code_rate / fs) % CODE_LENGTH as f64;
            let ci = cp as usize % CODE_LENGTH;
            let chip = self.code[ci] as f64;

            // Early
            let ei = ((cp - spc + CODE_LENGTH as f64) as usize) % CODE_LENGTH;
            e += wiped * Complex::new(self.code[ei] as f64, 0.0);

            // Prompt
            p += wiped * Complex::new(chip, 0.0);

            // Late
            let li = ((cp + spc) as usize) % CODE_LENGTH;
            l += wiped * Complex::new(self.code[li] as f64, 0.0);

            // Phase increment
            self.carrier_phase_rad += self.carrier_freq_hz / fs;
            self.carrier_phase_rad -= self.carrier_phase_rad.floor();
        }
        (e, p, l)
    }

    fn update_cn0(&mut self, prompt: Complex<f64>) {
        let wbp = prompt.norm_sqr();
        let nbp = prompt.re * prompt.re;
        let a = 0.01;
        self.cn0_wbp = (1.0 - a) * self.cn0_wbp + a * wbp;
        self.cn0_nbp = (1.0 - a) * self.cn0_nbp + a * nbp;
        self.cn0_n += 1;
    }
}
