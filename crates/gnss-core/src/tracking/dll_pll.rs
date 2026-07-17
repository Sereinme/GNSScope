//! GPS L1 C/A tracking — exact GNSS-SDR Tracking_FLL_PLL_filter port.
//! 3rd-order PLL (b3=2.4, a3=1.1, a2=1.414), no FLL, direct Doppler assignment.

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
    pub carrier_aiding: bool, pub coherent_ms: u32,
    pub pll_order: u32,
}
impl Default for TrackingConfig {
    fn default() -> Self {
        Self { fs: 4_000_000.0, if_freq: 0.0,
            dll_bw: 4.0, pll_bw: 30.0, fll_bw: 0.0,
            damping: 0.7, early_late_spc: 0.5, carrier_aiding: true,
            coherent_ms: 1, pll_order: 3,
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
    pll_filter: LoopFilter,
    fll_filt: f64,
    // Coherent accumulator
    coh_count: u32,
    coh_early: Complex<f64>, coh_prompt: Complex<f64>, coh_late: Complex<f64>,
    // GNSS-SDR Tracking_FLL_PLL_filter state (3rd order)
    pll_x: f64,  // initialized to 2*acq_doppler
    pll_w: f64,  // initialized to 0
    // Filter constants
    w0p: f64, w0p2: f64, w0p3: f64,
    b3: f64, a3: f64, a2: f64,
    w0f: f64, w0f2: f64,
    prev_prompt: Complex<f64>,
    cn0_i2: f64, cn0_q2: f64, cn0_n: u32,
    tracking_ms: u64,
    pull_in_time_ms: u64,
    pll_bw_pull_in: f64,
    w0p_pull_in: f64, w0p2_pull_in: f64, w0p3_pull_in: f64,
    last_prompt: Complex<f64>,
    symbol_acc: Complex<f64>, symbol_cnt: u32,
    cycle_ms: usize, bit_boundary: Option<usize>,
    bs_buf: [f64; 40], bs_idx: usize,
    bs_energy: [f64; 20], bs_count: u32,
    pub last_pll_in: f64, pub last_fll_in: f64,
}

impl DllPllTracking {
    pub fn new(prn: usize, doppler_hz: f64, code_phase_chips: f64, config: TrackingConfig) -> Self {
        let t = 0.001;
        let w0p = config.pll_bw / 0.7845;
        let w0p2 = w0p * w0p;
        let w0p3 = w0p2 * w0p;
        Self {
            config: config.clone(), code: &PRN_CODES[prn], t,
            carrier_freq_hz: config.if_freq + doppler_hz, carrier_phase_rad: 0.0,
            code_phase_chips, code_freq_chips_per_s: GPS_L1_CA_CODE_RATE,
            dll_filter: LoopFilter::new(config.dll_bw, config.damping, 1.0, t),
            pll_filter: {
                let mut f = LoopFilter::new_order(config.pll_bw, t, config.pll_order);
                f.initialize(doppler_hz);
                f
            },
            fll_filt: 0.0,
            coh_count: 0,
            coh_early: Complex::new(0.0, 0.0), coh_prompt: Complex::new(0.0, 0.0), coh_late: Complex::new(0.0, 0.0),
            // 2nd-order PLL init: old_nco = acq_doppler, old_error = 0
            pll_x: 0.0,  // old_carr_error
            pll_w: doppler_hz,  // old_carr_nco
            w0p, w0p2, w0p3,
            b3: 2.400, a3: 1.100, a2: 1.414,
            w0f: config.fll_bw / 0.53, w0f2: (config.fll_bw / 0.53).powi(2),
            prev_prompt: Complex::new(0.0, 0.0),
            cn0_i2: 0.0, cn0_q2: 0.0, cn0_n: 0,
            tracking_ms: 0,
            pull_in_time_ms: 4000,  // 4 seconds pull-in
            pll_bw_pull_in: 50.0,
            w0p_pull_in: 50.0_f64 / 0.7845,
            w0p2_pull_in: (50.0_f64 / 0.7845).powi(2),
            w0p3_pull_in: (50.0_f64 / 0.7845).powi(3),
            last_prompt: Complex::new(0.0, 0.0),
            symbol_acc: Complex::new(0.0, 0.0), symbol_cnt: 0,
            cycle_ms: 0, bit_boundary: None,
            bs_buf: [0.0; 40], bs_idx: 0,
            bs_energy: [0.0; 20], bs_count: 0,
            last_pll_in: 0.0, last_fll_in: 0.0,
        }
    }

    pub fn process(&mut self, samples: &[Complex<f64>]) -> TrackingOutput {
        let fs = self.config.fs;
        let spc_chips = self.config.early_late_spc;
        let spc_s = spc_chips * fs / self.code_freq_chips_per_s;
        let (e, p, l) = self.correlate(samples, spc_s);

        // Coherent accumulator (N ms before loop update)
        self.coh_early += e;
        self.coh_prompt += p;
        self.coh_late += l;
        self.coh_count += 1;

        let t_coh = self.t * self.config.coherent_ms as f64;
        let do_update = self.coh_count >= self.config.coherent_ms;

        if do_update {
            let ce = self.coh_early;
            let cp = self.coh_prompt;
            let cl = self.coh_late;
            self.coh_early = Complex::new(0.0, 0.0);
            self.coh_prompt = Complex::new(0.0, 0.0);
            self.coh_late = Complex::new(0.0, 0.0);
            self.coh_count = 0;

            // DLL (update every coherent_ms)
            let dll_err = discriminators::dll_discriminator(ce, cl, spc_chips);
            let dll_out = self.dll_filter.update(-dll_err);
            self.code_freq_chips_per_s = GPS_L1_CA_CODE_RATE - dll_out;
            if self.config.carrier_aiding {
                self.code_freq_chips_per_s += (self.carrier_freq_hz - self.config.if_freq)
                    * GPS_L1_CA_CODE_RATE / GPS_L1_FREQ;
            }

            // PLL: 2nd-order (tracking_2nd_PLL_filter.cc)
            let pll_disc = discriminators::pll_discriminator(cp) / TAU;
            self.last_pll_in = pll_disc;
            self.prev_prompt = cp;

            let k1 = self.w0f / self.w0p2;
            let k2 = t_coh / (2.0 * 1.0 / self.w0p2);
            let pll_nco = self.pll_w
                + k1 * (pll_disc - self.pll_x)
                + k2 * (pll_disc + self.pll_x);
            self.pll_x = pll_disc;
            self.pll_w = pll_nco;

            self.carrier_freq_hz = self.config.if_freq + pll_nco;
        }

        // Code NCO advances every 1ms
        self.code_phase_chips += self.code_freq_chips_per_s * self.t;
        self.code_phase_chips %= CODE_LENGTH as f64;

        let doppler = self.carrier_freq_hz - self.config.if_freq;
        self.update_cn0(p);
        self.last_prompt = p;
        let cn0 = self.compute_cn0();

        TrackingOutput {
            prompt: p, early_mag: e.norm(), late_mag: l.norm(),
            code_phase_chips: self.code_phase_chips,
            carrier_doppler_hz: doppler,
            cn0_db_hz: cn0.max(0.0),
        }
    }

    /// Output 1ms prompt value for external bit sync + 20ms accumulation.
    /// Returns Some(prompt) every call — caller does 20ms grouping.
    pub fn take_symbol(&mut self) -> Option<Complex<f64>> {
        // Return last prompt — caller accumulates 20 values
        Some(self.last_prompt)
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
        self.bs_buf[self.bs_idx % 40] = prompt.re;
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
