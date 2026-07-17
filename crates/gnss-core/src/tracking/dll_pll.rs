//! DLL + PLL/FLL tracking for GPS L1 C/A.
//!
//! Based on GNSS-SDR `dll_pll_veml_tracking.cc` + `Tracking_FLL_PLL_filter`.
//! Default: 2nd-order PLL with 1st-order FLL assist (FLL 35Hz, PLL 20Hz).

use super::discriminators;
use super::loop_filter::LoopFilter;
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

#[derive(Debug, Clone)]
pub struct TrackingConfig {
    pub fs: f64, pub if_freq: f64,
    pub dll_bw: f64, pub pll_bw: f64, pub fll_bw: f64,
    pub damping: f64, pub early_late_spc: f64,
}
impl Default for TrackingConfig {
    fn default() -> Self {
        Self { fs: 4_000_000.0, if_freq: 0.0,
            dll_bw: 4.0, pll_bw: 40.0, fll_bw: 0.0,
            damping: 0.7, early_late_spc: 0.5,
        }
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
    dll_filter: LoopFilter,
    pll_filter: LoopFilter,  // used when fll_bw == 0
    // FLL+PLL state (when fll_bw > 0)
    fll_pll_w: f64,
    prev_prompt: Option<Complex<f64>>,
    // FLL+PLL filter constants
    fp_w0p: f64, fp_w0p2: f64, fp_w0f: f64, fp_a2: f64,
    // C/N₀
    cn0_i2: f64, cn0_q2: f64, cn0_n: u32,
    // Symbol accumulation
    symbol_acc: Complex<f64>, symbol_cnt: u32,
    // Bit sync removed — handled externally via 20-phase decoder
    bs_buf: [f64; 40], bs_idx: usize,
    bs_energy: [f64; 20], bs_count: u32,
    pub bit_boundary: Option<usize>,
    bs_ms: usize,
}

impl DllPllTracking {
    pub fn new(prn: usize, doppler_hz: f64, code_phase_chips: f64, config: TrackingConfig) -> Self {
        let t = 0.001;
        // FLL+PLL filter constants (GNSS-SDR formula)
        let fp_w0p = config.pll_bw / 0.53;
        let fp_w0p2 = fp_w0p * fp_w0p;
        let fp_w0f = config.fll_bw / 0.25;
        let fp_a2 = 1.414;
        Self {
            config: config.clone(), code: &PRN_CODES[prn], t,
            carrier_freq_hz: config.if_freq + doppler_hz, carrier_phase_rad: 0.0,
            code_phase_chips, code_freq_chips_per_s: 1.023e6,
            dll_filter: LoopFilter::new(config.dll_bw, config.damping, 1.0, t),
            pll_filter: LoopFilter::new(config.pll_bw, config.damping, 0.25, t),
            fll_pll_w: doppler_hz,  // initialize with acquisition Doppler
            prev_prompt: None,
            fp_w0p, fp_w0p2, fp_w0f, fp_a2,
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

        // DLL
        let dll_err = discriminators::dll_discriminator(early, late, spc_chips);
        let dll_corr = self.dll_filter.update(-dll_err);
        self.code_freq_chips_per_s = 1.023e6 + dll_corr;
        self.code_phase_chips += self.code_freq_chips_per_s * self.t;
        self.code_phase_chips %= CODE_LENGTH as f64;

        // Carrier tracking: FLL+PLL or pure PLL
        let pll_disc = discriminators::pll_discriminator(prompt);
        if self.config.fll_bw > 0.0 {
            let fll_disc = if let Some(prev) = self.prev_prompt {
                discriminators::fll_discriminator(prev, prompt, self.t)
            } else { 0.0 };
            self.prev_prompt = Some(prompt);
            // GNSS-SDR 2nd-order PLL + 1st-order FLL update
            let pll_w_new = self.fll_pll_w
                + pll_disc * self.fp_w0p2 * self.t
                + fll_disc * self.fp_w0f * self.t;
            let carrier_err_hz = 0.5 * (pll_w_new + self.fll_pll_w)
                + self.fp_a2 * self.fp_w0p * pll_disc;
            self.fll_pll_w = pll_w_new;
            self.carrier_freq_hz += carrier_err_hz * self.t;
        } else {
            // Pure PLL: use existing loop filter
            let pll_corr_rad_per_s2 = self.pll_filter.update(pll_disc);
            let pll_corr_hz_per_s = pll_corr_rad_per_s2 / TAU;
            self.carrier_freq_hz += pll_corr_hz_per_s * self.t;
        }
        let doppler = self.carrier_freq_hz - self.config.if_freq;

        // C/N₀
        self.update_cn0(prompt);
        let cn0 = self.compute_cn0();

        // Symbol accumulation
        self.symbol_acc += prompt;
        self.symbol_cnt += 1;

        TrackingOutput {
            prompt, early_mag: early.norm(), late_mag: late.norm(),
            code_phase_chips: self.code_phase_chips,
            carrier_doppler_hz: doppler,
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
        let a = 0.01;
        self.cn0_i2 = (1.0 - a) * self.cn0_i2 + a * prompt.re * prompt.re;
        self.cn0_q2 = (1.0 - a) * self.cn0_q2 + a * prompt.im * prompt.im;
        self.cn0_n += 1;
    }

    fn compute_cn0(&self) -> f64 {
        if self.cn0_n < 20 || self.cn0_q2 < 1e-30 { return 0.0; }
        let ratio = self.cn0_i2 / self.cn0_q2;
        if ratio > 1.1 { 10.0 * (ratio - 1.0).log10() + 27.0 } else { 0.0 }
    }
}
