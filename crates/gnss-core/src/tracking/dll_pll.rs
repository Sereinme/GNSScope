//! DLL + PLL/FLL tracking. GNSS-SDR compatible algorithm.
//!
//! Key design (from GNSS-SDR run_dll_pll + update_tracking_vars):
//! - PLL input: atan(Q/I) / 2π (cycles), FLL input: fll_diff_atan / 2π (Hz)
//! - Carrier Doppler: DIRECT ASSIGNMENT from filter output (not accumulation)
//! - Code freq: nominal - DLL filter output + carrier aiding
//! - Filter initialized with acquisition Doppler

use super::discriminators;
use super::loop_filter::LoopFilter;
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

const GPS_L1_FREQ: f64 = 1575.42e6;
const GPS_L1_CA_CODE_RATE: f64 = 1.023e6;

#[derive(Debug, Clone)]
pub struct TrackingConfig {
    pub fs: f64, pub if_freq: f64,
    pub dll_bw: f64, pub pll_bw: f64, pub fll_bw: f64,
    pub damping: f64, pub early_late_spc: f64,
    pub carrier_aiding: bool,
    /// Coherent integration time in ms (1, 2, 5, 10, 20)
    pub coherent_ms: u32,
}
impl Default for TrackingConfig {
    fn default() -> Self {
        Self { fs: 4_000_000.0, if_freq: 0.0,
            dll_bw: 4.0, pll_bw: 10.0, fll_bw: 10.0,
            damping: 0.7, early_late_spc: 0.5, carrier_aiding: true,
            coherent_ms: 5,  // reduce FLL noise
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
    // Coherent accumulator
    coh_count: u32,
    coh_early: Complex<f64>, coh_prompt: Complex<f64>, coh_late: Complex<f64>,
    // GNSS-SDR FLL+PLL filter state
    pll_w: f64,           // frequency (initialized = acq Doppler)
    fp_w0p2: f64, fp_w0f: f64, fp_w0p: f64, fp_a2: f64,
    prev_prompt: Complex<f64>,
    prev_pll_in: f64,     // previous PLL input (cycles)
    fll_filt: f64,        // EWMA-smoothed FLL discriminator
    pub last_pll_in: f64,  // last PLL discriminator (cycles)
    pub last_fll_in: f64,  // last FLL discriminator (Hz)
    /// Pull-in: pure FLL for first N seconds, then FLL+PLL
    tracking_ms: u64,
    pull_in_time_ms: u64,
    // C/N₀
    cn0_i2: f64, cn0_q2: f64, cn0_n: u32,
    // Symbol accumulation (aligned to bit boundary)
    symbol_acc: Complex<f64>, symbol_cnt: u32,
    cycle_ms: usize, bit_boundary: Option<usize>,
    bs_buf: [f64; 40], bs_idx: usize,
    bs_energy: [f64; 20], bs_count: u32,
}

impl DllPllTracking {
    pub fn new(prn: usize, doppler_hz: f64, code_phase_chips: f64, config: TrackingConfig) -> Self {
        let t = 0.001;
        // GNSS-SDR filter constants
        let w0p = config.pll_bw / 0.53;
        let w0p2 = w0p * w0p;
        let w0f = config.fll_bw / 0.25;
        let a2 = 1.414;
        Self {
            config: config.clone(), code: &PRN_CODES[prn], t,
            carrier_freq_hz: config.if_freq + doppler_hz, carrier_phase_rad: 0.0,
            code_phase_chips, code_freq_chips_per_s: GPS_L1_CA_CODE_RATE,
            // DLL filter T = coherent_ms * 1ms
            dll_filter: LoopFilter::new(config.dll_bw, config.damping, 1.0, t * config.coherent_ms as f64),
            // Coherent accumulator
            coh_count: 0,
            coh_early: Complex::new(0.0, 0.0), coh_prompt: Complex::new(0.0, 0.0), coh_late: Complex::new(0.0, 0.0),
            // FLL+PLL filter
            pll_w: doppler_hz,
            fp_w0p2: w0p2, fp_w0f: w0f, fp_w0p: w0p, fp_a2: a2,
            prev_prompt: Complex::new(0.0, 0.0),
            prev_pll_in: 0.0,
            fll_filt: 0.0,
            last_pll_in: 0.0, last_fll_in: 0.0,
            tracking_ms: 0,
            pull_in_time_ms: 4000,  // 4 seconds pure FLL pull-in
            cn0_i2: 0.0, cn0_q2: 0.0, cn0_n: 0,
            symbol_acc: Complex::new(0.0, 0.0), symbol_cnt: 0,
            cycle_ms: 0, bit_boundary: None,
            bs_buf: [0.0; 40], bs_idx: 0,
            bs_energy: [0.0; 20], bs_count: 0,
        }
    }

    pub fn process(&mut self, samples: &[Complex<f64>]) -> TrackingOutput {
        let fs = self.config.fs;
        let spc_chips = self.config.early_late_spc;
        let spc_s = spc_chips * fs / self.code_freq_chips_per_s;
        let (e, p, l) = self.correlate(samples, spc_s);
        let t_coh = self.t * self.config.coherent_ms as f64;

        // --- Coherent accumulation ---
        self.coh_early += e;
        self.coh_prompt += p;
        self.coh_late += l;
        self.coh_count += 1;

        // --- Symbol accumulation (always per-1ms, aligned to bit boundary) ---
        self.update_bit_sync(p);
        self.cycle_ms = (self.cycle_ms + 1) % 20;
        let boundary = self.bit_boundary.unwrap_or(0);
        if self.cycle_ms == boundary && self.symbol_cnt > 0 && self.symbol_cnt < 20 {
            self.symbol_acc = Complex::new(0.0, 0.0);
            self.symbol_cnt = 0;
        }
        if self.cycle_ms == boundary || self.symbol_cnt > 0 {
            self.symbol_acc += p;
            self.symbol_cnt += 1;
        }

        // --- Loop updates only every coherent_ms ---
        if self.coh_count >= self.config.coherent_ms {
            let early = self.coh_early;
            let prompt = self.coh_prompt;
            let late = self.coh_late;
            self.tracking_ms += self.config.coherent_ms as u64;

            // DLL
            let dll_err = discriminators::dll_discriminator(early, late, spc_chips);
            let dll_out = self.dll_filter.update(-dll_err);
            self.code_freq_chips_per_s = GPS_L1_CA_CODE_RATE - dll_out;
            if self.config.carrier_aiding {
                self.code_freq_chips_per_s += (self.carrier_freq_hz - self.config.if_freq)
                    * GPS_L1_CA_CODE_RATE / GPS_L1_FREQ;
            }

            // PLL (input in cycles) — disabled during pull-in (pure FLL)
            let pll_in_raw = discriminators::pll_discriminator(prompt) / TAU;
            self.last_pll_in = pll_in_raw;
            let pll_in = if self.tracking_ms < self.pull_in_time_ms { 0.0 } else { pll_in_raw };
            // FLL (input in Hz), with EWMA smoothing for noise reduction
            let fll_raw = if self.config.fll_bw > 0.0 {
                discriminators::fll_discriminator(self.prev_prompt, prompt, t_coh)
            } else { 0.0 };
            self.last_fll_in = fll_raw;
            let alpha = 0.02;  // EWMA smoothing factor (stronger for 1ms noise)
            self.fll_filt = (1.0 - alpha) * self.fll_filt + alpha * fll_raw;
            let fll_in = self.fll_filt;  // use smoothed value
            self.prev_prompt = prompt;

            // FLL weight: full during pull-in, reduced after
            let fll_weight = if self.tracking_ms < self.pull_in_time_ms { 1.0 } else { 0.1 };

            // GNSS-SDR FLL+PLL formula
            let pll_w_new = self.pll_w
                + pll_in * self.fp_w0p2 * t_coh
                - fll_in * self.fp_w0f * t_coh * fll_weight;
            let carrier_doppler = 0.5 * (pll_w_new + self.pll_w)
                + self.fp_a2 * self.fp_w0p * pll_in;
            self.pll_w = pll_w_new;
            self.prev_pll_in = pll_in;
            self.carrier_freq_hz = self.config.if_freq + carrier_doppler;

            // Code NCO advance
            self.code_phase_chips += self.code_freq_chips_per_s * t_coh;
            self.code_phase_chips %= CODE_LENGTH as f64;

            // Reset coherent accumulator
            self.coh_early = Complex::new(0.0, 0.0);
            self.coh_prompt = Complex::new(0.0, 0.0);
            self.coh_late = Complex::new(0.0, 0.0);
            self.coh_count = 0;
        }

        let doppler = self.carrier_freq_hz - self.config.if_freq;
        self.update_cn0(p);
        let cn0 = self.compute_cn0();

        TrackingOutput {
            prompt: p, early_mag: e.norm(), late_mag: l.norm(),
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

    fn update_bit_sync(&mut self, prompt: Complex<f64>) {
        if self.bit_boundary.is_some() { return; }
        let i = prompt.re;
        self.bs_buf[self.bs_idx % 40] = i;
        self.bs_idx += 1;
        if self.bs_idx < 20 { return; }
        let start = self.bs_idx - 20;
        for b in 0..20 {
            let b0 = (start + b) % 40;
            let b1 = (b0 + 19) % 40;
            let sum: f64 = if b1 >= b0 {
                self.bs_buf[b0..=b1].iter().sum()
            } else {
                self.bs_buf[b0..].iter().sum::<f64>() + self.bs_buf[..=b1].iter().sum::<f64>()
            };
            self.bs_energy[b] += sum.abs();
        }
        self.bs_count += 1;
        if self.bs_count >= 40 {
            let mut best = 0; let mut best_val = 0.0f64;
            for (i, &e) in self.bs_energy.iter().enumerate() {
                if e > best_val { best_val = e; best = i; }
            }
            self.bit_boundary = Some(best);
        }
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
