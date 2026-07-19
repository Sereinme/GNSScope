//! GPS L1 C/A tracking — exact GNSS-SDR Tracking_FLL_PLL_filter port.
//! 3rd-order PLL (b3=2.4, a3=1.1, a2=1.414), no FLL, direct Doppler assignment.

use super::discriminators;
use super::loop_filter::LoopFilter;
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::collections::VecDeque;
use std::f64::consts::TAU;

const GPS_L1_FREQ: f64 = 1575.42e6;
const GPS_L1_CA_CODE_RATE: f64 = 1.023e6;

#[derive(Debug, Clone)]
pub struct TrackingConfig {
    pub fs: f64,
    pub if_freq: f64,
    pub dll_bw: f64,
    pub pll_bw: f64,
    pub fll_bw: f64,
    pub damping: f64,
    pub early_late_spc: f64,
    pub carrier_aiding: bool,
    pub coherent_ms: u32,
    pub pll_order: u32,
    /// Prompt correlator samples used by the narrowband/wideband C/N0 estimator.
    pub cn0_samples: usize,
    /// Exponential smoothing factor for C/N0 in dB-Hz.
    pub cn0_smoother_alpha: f64,
}
impl Default for TrackingConfig {
    fn default() -> Self {
        Self {
            fs: 4_000_000.0,
            if_freq: 0.0,
            dll_bw: 4.0,
            pll_bw: 50.0,
            fll_bw: 0.0,
            damping: 0.7,
            early_late_spc: 0.5,
            carrier_aiding: true,
            coherent_ms: 1,
            pll_order: 3,
            cn0_samples: 20,
            cn0_smoother_alpha: 0.1,
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
    /// Prompt-phase coherence metric in the range 0..1.
    ///
    /// A value near 1 means the prompt correlator phase is stable over the
    /// current lock window; values near 0 indicate phase scatter or loss of
    /// carrier lock.
    pub pll_lock_metric: f64,
    /// Integration-boundary sample counter since tracking start.
    pub sample_counter: u64,
    /// Code-aligned integer receiver sample tag for observables.
    ///
    /// Use `tracking_sample_counter + rem_code_phase_samples` for the
    /// GNSS-SDR-style receiver timestamp.
    pub tracking_sample_counter: u64,
    /// Sub-sample code phase residual in samples, < 1.0.
    pub rem_code_phase_samples: f64,
    /// 0–19: which 1ms epoch within the current 20ms data bit.
    pub ms_within_bit: u8,
    /// DLL discriminator output (chips). Positive = local leads signal.
    /// Use with code_phase_chips: true_cp = code_phase_chips - dll_error.
    pub dll_error: f64,
}

pub struct DllPllTracking {
    config: TrackingConfig,
    code: &'static [i8; CODE_LENGTH],
    t: f64,
    carrier_freq_hz: f64,
    carrier_phase_rad: f64,
    code_phase_chips: f64,
    code_freq_chips_per_s: f64,
    dll_filter: LoopFilter,
    // Coherent accumulator (N ms before loop update)
    coh_count: u32,
    coh_early: Complex<f64>,
    coh_prompt: Complex<f64>,
    coh_late: Complex<f64>,
    // GNSS-SDR Tracking_FLL_PLL_filter state (carrier loop).
    // 3rd order: pll_x = 2*acq_doppler, pll_w = 0
    // 2nd order: pll_w = acq_doppler,   pll_x = 0
    pll_x: f64,
    pll_w: f64,
    // Filter constants (order-dependent, see set_params)
    w0p: f64,
    w0p2: f64,
    w0p3: f64,
    b3: f64,
    a3: f64,
    a2: f64,
    prev_prompt: Complex<f64>,
    cn0_prompt_buffer: VecDeque<Complex<f64>>,
    cn0_smoothed_db_hz: Option<f64>,
    last_prompt: Complex<f64>,
    cycle_ms: usize,
    bit_boundary: Option<usize>,
    pub last_pll_in: f64,
    pub last_fll_in: f64,
    /// Absolute sample count since tracking start, incremented by
    /// correlation_length_samples per integration period (GNSS-SDR).
    sample_counter: u64,
    /// Last DLL discriminator output in chips (positive = local leads signal).
    last_dll_error: f64,
    /// 0–19: which 1-ms epoch within the current 20-ms data bit.
    ms_within_bit: u8,
}

impl DllPllTracking {
    pub fn new(
        prn: usize,
        doppler_hz: f64,
        code_phase_chips: f64,
        config: TrackingConfig,
        start_sample_index: u64,
    ) -> Self {
        let t = 0.001;
        // Tracking_FLL_PLL_filter::set_params — natural frequencies depend on order.
        //   3rd order: w0p = pll_bw/0.7845
        //   2nd order: w0p = pll_bw/0.53
        let w0p = if config.pll_order >= 3 {
            config.pll_bw / 0.7845
        } else {
            config.pll_bw / 0.53
        };
        let w0p2 = w0p * w0p;
        let w0p3 = w0p2 * w0p;
        // Tracking_FLL_PLL_filter::initialize — accumulator init depends on order.
        //   3rd order: pll_x = 2*acq_doppler, pll_w = 0
        //   2nd order: pll_w = acq_doppler,   pll_x = 0
        let (pll_x_init, pll_w_init) = if config.pll_order >= 3 {
            (2.0 * doppler_hz, 0.0)
        } else {
            (0.0, doppler_hz)
        };
        Self {
            config: config.clone(),
            code: &PRN_CODES[prn],
            t,
            carrier_freq_hz: config.if_freq + doppler_hz,
            carrier_phase_rad: 0.0,
            code_phase_chips,
            code_freq_chips_per_s: GPS_L1_CA_CODE_RATE,
            dll_filter: LoopFilter::new(config.dll_bw, config.damping, 1.0, t),
            coh_count: 0,
            coh_early: Complex::new(0.0, 0.0),
            coh_prompt: Complex::new(0.0, 0.0),
            coh_late: Complex::new(0.0, 0.0),
            pll_x: pll_x_init,
            pll_w: pll_w_init,
            w0p,
            w0p2,
            w0p3,
            b3: 2.400,
            a3: 1.100,
            a2: 1.414,
            prev_prompt: Complex::new(0.0, 0.0),
            cn0_prompt_buffer: VecDeque::with_capacity(config.cn0_samples.max(1)),
            cn0_smoothed_db_hz: None,
            last_prompt: Complex::new(0.0, 0.0),
            cycle_ms: 0,
            bit_boundary: None,
            last_pll_in: 0.0,
            last_fll_in: 0.0,
            sample_counter: start_sample_index,
            last_dll_error: 0.0,
            ms_within_bit: 0,
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

            // DLL (update every coherent_ms).
            // dll_discriminator returns spc*(E-L)/(E+L): positive when local code
            // leads (E>L), negative when it lags (E<L). GNSS-SDR convention:
            //   code_freq = chip_rate - dll_error_filt
            // A lead (disc>0) must slow the code → filt>0 → code_freq decreases.
            // So the discriminator feeds the filter directly (no sign flip).
            let dll_err = discriminators::dll_discriminator(ce, cl, spc_chips);
            self.last_dll_error = dll_err;
            let dll_out = self.dll_filter.update(dll_err);
            self.code_freq_chips_per_s = GPS_L1_CA_CODE_RATE - dll_out;
            if self.config.carrier_aiding {
                self.code_freq_chips_per_s += (self.carrier_freq_hz - self.config.if_freq)
                    * GPS_L1_CA_CODE_RATE
                    / GPS_L1_FREQ;
            }

            // PLL: exact port of GNSS-SDR Tracking_FLL_PLL_filter::get_carrier_error
            // (FLL_discriminator = 0 in PLL-only mode). Input is the normalized phase
            // error atan(Q/I)/(2π); output is the absolute carrier Doppler in Hz.
            let pll_disc = discriminators::pll_discriminator(cp) / TAU;
            self.last_pll_in = pll_disc;
            self.prev_prompt = cp;

            let pll_nco = if self.config.pll_order >= 3 {
                // 3rd-order PLL with 2nd-order FLL assist.
                // FLL term (w0f2 * FLL_disc) vanishes when fll_bw = 0.
                self.pll_w += t_coh * (self.w0p3 * pll_disc);
                self.pll_x += t_coh * (0.5 * self.pll_w + self.a3 * self.w0p2 * pll_disc);
                0.5 * self.pll_x + self.b3 * self.w0p * pll_disc
            } else {
                // 2nd-order PLL with 1st-order FLL assist.
                let pll_w_new = self.pll_w + pll_disc * self.w0p2 * t_coh;
                let nco = 0.5 * (pll_w_new + self.pll_w) + self.a2 * self.w0p * pll_disc;
                self.pll_w = pll_w_new;
                nco
            };
            self.carrier_freq_hz = self.config.if_freq + pll_nco;
        }

        // Code NCO advances every 1ms
        self.code_phase_chips += self.code_freq_chips_per_s * self.t;
        self.code_phase_chips %= CODE_LENGTH as f64;

        // GNSS-SDR: increment absolute sample counter (1 ms worth of samples).
        self.sample_counter += (self.config.fs / 1000.0) as u64;
        let code_phase_samples = self.code_phase_chips * self.config.fs / GPS_L1_CA_CODE_RATE;
        let code_phase_integer_samples = code_phase_samples.floor() as u64;
        let rem_code_phase_samples = code_phase_samples - code_phase_integer_samples as f64;
        let tracking_sample_counter = self.sample_counter + code_phase_integer_samples;

        // Advance ms_within_bit: 0→19 wraps at bit boundary.
        if let Some(boundary) = self.bit_boundary {
            // boundary is which 1ms epoch (0-19) within the 20ms cycle is the
            // first ms of a data bit. At that epoch, reset to 0; otherwise +1.
            if self.cycle_ms % 20 == boundary {
                self.ms_within_bit = 0;
            } else {
                self.ms_within_bit = (self.ms_within_bit + 1) % 20;
            }
        } else {
            self.ms_within_bit = (self.ms_within_bit + 1) % 20;
        }
        self.cycle_ms = self.cycle_ms.wrapping_add(1);

        let doppler = self.carrier_freq_hz - self.config.if_freq;
        self.update_cn0(p);
        self.last_prompt = p;
        let cn0 = self.compute_cn0();
        let pll_lock_metric = self.compute_pll_lock_metric();

        TrackingOutput {
            prompt: p,
            early_mag: e.norm(),
            late_mag: l.norm(),
            code_phase_chips: self.code_phase_chips,
            carrier_doppler_hz: doppler,
            cn0_db_hz: cn0.max(0.0),
            pll_lock_metric,
            sample_counter: self.sample_counter,
            tracking_sample_counter,
            rem_code_phase_samples,
            ms_within_bit: self.ms_within_bit,
            dll_error: self.last_dll_error,
        }
    }

    /// Output 1ms prompt value for external bit sync + 20ms accumulation.
    /// Returns Some(prompt) every call — caller does 20ms grouping.
    pub fn take_symbol(&mut self) -> Option<Complex<f64>> {
        // Return last prompt — caller accumulates 20 values
        Some(self.last_prompt)
    }

    pub fn code_freq(&self) -> f64 {
        self.code_freq_chips_per_s
    }
    pub fn dll_state(&self) -> f64 {
        self.dll_filter.state()
    }
    pub fn carrier_phase_rad(&self) -> f64 {
        self.carrier_phase_rad
    }
    pub fn carrier_freq_hz(&self) -> f64 {
        self.carrier_freq_hz
    }

    /// Set the 20ms data-bit boundary (0-19) from external bit synchronizer.
    /// After this is called, `ms_within_bit` in TrackingOutput will be aligned
    /// so that 0 corresponds to the first 1ms epoch of a data bit.
    pub fn set_bit_boundary(&mut self, boundary: usize) {
        self.set_absolute_bit_boundary(boundary);
    }

    /// Set bit boundary returned by a synchronizer whose epoch started at
    /// `sync_start_ms` after this tracker started.
    pub fn set_bit_boundary_from_sync(&mut self, boundary: usize, sync_start_ms: usize) {
        self.set_absolute_bit_boundary(sync_start_ms.wrapping_add(boundary) % 20);
    }

    fn set_absolute_bit_boundary(&mut self, boundary: usize) {
        self.bit_boundary = Some(boundary % 20);
        self.cn0_prompt_buffer.clear();
        self.cn0_smoothed_db_hz = None;
    }

    fn correlate(
        &mut self,
        samples: &[Complex<f64>],
        spc_s: f64,
    ) -> (Complex<f64>, Complex<f64>, Complex<f64>) {
        let fs = self.config.fs;
        let spc_chips = spc_s / fs * self.code_freq_chips_per_s;
        let mut e = Complex::new(0.0, 0.0);
        let mut p = Complex::new(0.0, 0.0);
        let mut l = Complex::new(0.0, 0.0);
        for (i, sample) in samples.iter().enumerate() {
            let carrier = Complex::new(
                (TAU * self.carrier_phase_rad).cos(),
                -(TAU * self.carrier_phase_rad).sin(),
            );
            let wiped = sample * carrier;
            let cp = (self.code_phase_chips + i as f64 * self.code_freq_chips_per_s / fs)
                % CODE_LENGTH as f64;
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
        let cn0_samples = self.cn0_window_len();
        if self.bit_boundary.is_some() && self.ms_within_bit == 0 {
            self.cn0_prompt_buffer.clear();
        }
        if self.cn0_prompt_buffer.len() == cn0_samples {
            self.cn0_prompt_buffer.pop_front();
        }
        self.cn0_prompt_buffer.push_back(prompt);

        if let Some(cn0_db_hz) = self.compute_cn0_nwpr() {
            let alpha = self.config.cn0_smoother_alpha.clamp(0.0, 1.0);
            let smoothed = match self.cn0_smoothed_db_hz {
                Some(prev) => alpha * cn0_db_hz + (1.0 - alpha) * prev,
                None => cn0_db_hz,
            };
            self.cn0_smoothed_db_hz = Some(smoothed);
        }
    }

    fn compute_cn0(&self) -> f64 {
        self.cn0_smoothed_db_hz.unwrap_or(0.0)
    }

    fn compute_pll_lock_metric(&self) -> f64 {
        let n = self.cn0_prompt_buffer.len();
        if n == 0 {
            return 0.0;
        }

        let coherent_sum: Complex<f64> = self.cn0_prompt_buffer.iter().copied().sum();
        let wideband_power: f64 = self.cn0_prompt_buffer.iter().map(|p| p.norm_sqr()).sum();
        if wideband_power <= 1e-30 {
            return 0.0;
        }

        (coherent_sum.norm_sqr() / (n as f64 * wideband_power)).clamp(0.0, 1.0)
    }

    fn compute_cn0_nwpr(&self) -> Option<f64> {
        let m = self.cn0_window_len();
        if self.cn0_prompt_buffer.len() < m {
            return None;
        }

        let coherent_sum: Complex<f64> = self.cn0_prompt_buffer.iter().copied().sum();
        let narrowband_power = coherent_sum.norm_sqr();
        let wideband_power: f64 = self.cn0_prompt_buffer.iter().map(|p| p.norm_sqr()).sum();
        if wideband_power <= 1e-30 {
            return None;
        }

        let m = m as f64;
        let nwpr = narrowband_power / wideband_power;
        if !(1.0..m).contains(&nwpr) {
            return None;
        }

        let snr = (nwpr - 1.0) / (m - nwpr);
        if snr <= 0.0 {
            return None;
        }

        // Each Prompt value is produced from one 1 ms code-period correlation.
        let cn0_hz = snr / self.t;
        Some(10.0 * cn0_hz.log10())
    }

    fn cn0_window_len(&self) -> usize {
        let configured = self.config.cn0_samples.max(2);
        if self.bit_boundary.is_some() {
            configured.min(20)
        } else {
            configured
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cn0_nwpr_known_prompt_snr() {
        let config = TrackingConfig {
            cn0_samples: 100,
            cn0_smoother_alpha: 1.0,
            ..Default::default()
        };
        let mut trk = DllPllTracking::new(0, 0.0, 0.0, config, 0);

        // Signal amplitude is 1.0. The deterministic zero-mean circular noise
        // has power 0.01, so SNR ~= 100 and C/N0 ~= 100 / 1 ms = 50 dB-Hz.
        for i in 0..100 {
            let phase = TAU * i as f64 / 100.0;
            let noise = Complex::new(0.1 * phase.cos(), 0.1 * phase.sin());
            trk.update_cn0(Complex::new(1.0, 0.0) + noise);
        }

        let cn0 = trk.compute_cn0();
        assert!((cn0 - 50.0).abs() < 0.2, "C/N0 = {cn0:.3} dB-Hz");
    }

    #[test]
    fn test_cn0_nwpr_window_resets_at_bit_boundary() {
        let config = TrackingConfig {
            cn0_samples: 20,
            cn0_smoother_alpha: 1.0,
            ..Default::default()
        };
        let mut trk = DllPllTracking::new(0, 0.0, 0.0, config, 0);
        trk.set_bit_boundary(0);

        for i in 0..20 {
            trk.ms_within_bit = i;
            let phase = TAU * i as f64 / 20.0;
            let noise = Complex::new(0.1 * phase.cos(), 0.1 * phase.sin());
            trk.update_cn0(Complex::new(1.0, 0.0) + noise);
        }
        let first_bit_cn0 = trk.compute_cn0();
        assert!(
            (first_bit_cn0 - 50.0).abs() < 1.0,
            "first bit C/N0 = {first_bit_cn0:.3}"
        );

        trk.ms_within_bit = 0;
        trk.update_cn0(Complex::new(-0.9, 0.0));
        assert_eq!(trk.cn0_prompt_buffer.len(), 1);
        assert_eq!(trk.compute_cn0(), first_bit_cn0);

        for i in 1..20 {
            trk.ms_within_bit = i;
            let phase = TAU * i as f64 / 20.0;
            let noise = Complex::new(0.1 * phase.cos(), 0.1 * phase.sin());
            trk.update_cn0(Complex::new(-1.0, 0.0) + noise);
        }
        let second_bit_cn0 = trk.compute_cn0();
        assert!(
            (second_bit_cn0 - 50.0).abs() < 1.0,
            "second bit C/N0 = {second_bit_cn0:.3}"
        );
    }

    #[test]
    fn test_cn0_nwpr_clamps_to_one_data_bit_after_sync() {
        let config = TrackingConfig {
            cn0_samples: 100,
            cn0_smoother_alpha: 1.0,
            ..Default::default()
        };
        let mut trk = DllPllTracking::new(0, 0.0, 0.0, config, 0);
        trk.set_bit_boundary(0);

        for i in 0..20 {
            trk.ms_within_bit = i;
            let phase = TAU * i as f64 / 20.0;
            trk.update_cn0(
                Complex::new(1.0, 0.0) + Complex::new(0.1 * phase.cos(), 0.1 * phase.sin()),
            );
        }

        assert_eq!(trk.cn0_prompt_buffer.len(), 20);
        assert!(trk.compute_cn0() > 0.0);
    }

    #[test]
    fn test_pll_lock_metric_tracks_prompt_phase_coherence() {
        let config = TrackingConfig {
            cn0_samples: 20,
            cn0_smoother_alpha: 1.0,
            ..Default::default()
        };
        let mut locked = DllPllTracking::new(0, 0.0, 0.0, config.clone(), 0);
        let mut unlocked = DllPllTracking::new(0, 0.0, 0.0, config, 0);

        for _ in 0..20 {
            locked.update_cn0(Complex::new(1.0, 0.0));
        }
        for i in 0..20 {
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            unlocked.update_cn0(Complex::new(sign, 0.0));
        }

        assert!(locked.compute_pll_lock_metric() > 0.99);
        assert!(unlocked.compute_pll_lock_metric() < 0.01);
    }

    #[test]
    fn test_bit_boundary_from_sync_converts_to_tracker_phase() {
        let config = TrackingConfig::default();
        let mut trk = DllPllTracking::new(0, 0.0, 0.0, config, 0);

        trk.set_bit_boundary_from_sync(7, 5);
        assert_eq!(trk.bit_boundary, Some(12));

        trk.cycle_ms = 11;
        trk.ms_within_bit = 10;
        let out = trk.process(&[]);
        assert_eq!(out.ms_within_bit, 11);

        trk.cycle_ms = 12;
        trk.ms_within_bit = 11;
        let out = trk.process(&[]);
        assert_eq!(out.ms_within_bit, 0);
    }

    #[test]
    fn test_tracking_sample_counter_includes_integer_code_phase() {
        let config = TrackingConfig::default();
        let mut trk = DllPllTracking::new(0, 0.0, 276.8, config, 10_000);

        let out = trk.process(&[]);
        let code_phase_samples = 276.8 * 4e6 / GPS_L1_CA_CODE_RATE;

        assert_eq!(out.sample_counter, 14_000);
        assert_eq!(
            out.tracking_sample_counter,
            out.sample_counter + code_phase_samples.floor() as u64
        );
        assert!((out.rem_code_phase_samples - code_phase_samples.fract()).abs() < 1e-9);
    }
}
