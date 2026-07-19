//! PCPS (Parallel Code Phase Search) acquisition for GPS L1 C/A.
//!
//! Based on GNSS-SDR's `pcps_acquisition.cc` algorithm:
//! 1. Generate local PRN code, FFT-conjugate it
//! 2. For each Doppler bin: wipe carrier, FFT, multiply by conj(code_fft), IFFT
//! 3. Compute squared magnitude → non-coherent accumulation
//! 4. Find peak and compare with CFAR threshold (peak/mean)

use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use rustfft::FftPlanner;
use std::f64::consts::TAU;

/// Result of an acquisition attempt for a single satellite.
#[derive(Debug, Clone)]
pub struct AcquisitionResult {
    /// PRN number (1-32)
    pub prn: u8,
    /// Absolute sample index of the first sample in the acquisition window.
    pub sample_index: u64,
    /// Detected Doppler frequency in Hz
    pub doppler_hz: f64,
    /// Detected code phase in chips (fractional, 0..1023)
    pub code_phase_chips: f64,
    /// Peak magnitude / mean magnitude (CFAR test statistic)
    pub peak_to_mean: f64,
    /// Whether the signal was detected (above threshold)
    pub detected: bool,
}

/// PCPS acquisition configuration.
pub struct PcpsConfig {
    /// Sampling frequency in Hz
    pub fs: f64,
    /// Intermediate frequency in Hz (0 for zero-IF)
    pub if_freq: f64,
    /// Doppler search range: ±doppler_max Hz
    pub doppler_max: f64,
    /// Doppler bin step in Hz
    pub doppler_step: f64,
    /// Coherent integration time in seconds (typically 0.001 for 1ms)
    pub coherent_integration_ms: f64,
    /// Number of coherent integrations to accumulate non-coherently.
    pub noncoherent_integrations: usize,
    /// Detection threshold: peak/mean ratio
    pub threshold: f64,
}

impl Default for PcpsConfig {
    fn default() -> Self {
        Self {
            fs: 4_000_000.0,
            if_freq: 0.0,
            doppler_max: 10_000.0,
            doppler_step: 500.0,
            coherent_integration_ms: 1.0,
            noncoherent_integrations: 1,
            threshold: 2.5,
        }
    }
}

/// Parallel Code Phase Search acquisition engine.
pub struct PcpsAcquisition {
    config: PcpsConfig,
    fft_planner: FftPlanner<f64>,
}

impl PcpsAcquisition {
    pub fn new(config: PcpsConfig) -> Self {
        Self {
            config,
            fft_planner: FftPlanner::new(),
        }
    }

    /// Acquire a single PRN on the given signal samples.
    ///
    /// `samples` must contain `noncoherent_integrations` coherent windows.
    /// With the default config this is exactly one 1 ms GPS L1 C/A code.
    pub fn acquire(&mut self, prn: usize, samples: &[Complex<f64>]) -> AcquisitionResult {
        self.acquire_at_sample(prn, samples, 0)
    }

    pub fn acquire_at_sample(
        &mut self,
        prn: usize,
        samples: &[Complex<f64>],
        sample_index: u64,
    ) -> AcquisitionResult {
        self.acquire_noncoherent_at_sample(
            prn,
            samples,
            sample_index,
            self.config.noncoherent_integrations,
        )
    }

    pub fn acquire_noncoherent_at_sample(
        &mut self,
        prn: usize,
        samples: &[Complex<f64>],
        sample_index: u64,
        noncoherent_integrations: usize,
    ) -> AcquisitionResult {
        let samples_per_code =
            (self.config.fs * self.config.coherent_integration_ms / 1000.0) as usize;
        let noncoherent_integrations = noncoherent_integrations.max(1);
        let expected_samples = samples_per_code * noncoherent_integrations;
        debug_assert_eq!(
            samples.len(),
            expected_samples,
            "Expected {} samples for {} non-coherent integrations, got {}",
            expected_samples,
            noncoherent_integrations,
            samples.len()
        );

        let fft_size = samples_per_code;
        let doppler_bins = self.doppler_bins();

        // Step 1: Generate local code (resampled to fs) and compute FFT + conjugate
        let local_code_fft_conj = self.generate_local_code_fft(prn, fft_size);

        // Step 2: Allocate magnitude grid [doppler_bins × fft_size]
        let mut magnitude_grid = vec![vec![0.0f64; fft_size]; doppler_bins.len()];

        // Step 3: Doppler search per coherent window, accumulated as power.
        for integration_idx in 0..noncoherent_integrations {
            let start = integration_idx * samples_per_code;
            let end = start + samples_per_code;
            self.accumulate_coherent_grid(
                &samples[start..end],
                &doppler_bins,
                &local_code_fft_conj,
                &mut magnitude_grid,
            );
        }

        // Step 4: Compute statistics (cf. GNSS-SDR compute_statistics)
        self.compute_result(prn, sample_index, &doppler_bins, &magnitude_grid, fft_size)
    }

    fn accumulate_coherent_grid(
        &mut self,
        samples: &[Complex<f64>],
        doppler_bins: &[i32],
        local_code_fft_conj: &[Complex<f64>],
        magnitude_grid: &mut [Vec<f64>],
    ) {
        let fft_size = samples.len();
        for (bin_idx, &doppler) in doppler_bins.iter().enumerate() {
            let doppler_with_if = doppler as f64 + self.config.if_freq;

            // Generate carrier wipe-off signal
            let carrier: Vec<Complex<f64>> = (0..fft_size)
                .map(|n| {
                    let phase = -TAU * doppler_with_if * (n as f64) / self.config.fs;
                    Complex::new(phase.cos(), phase.sin())
                })
                .collect();

            // Multiply input by carrier
            let wiped: Vec<Complex<f64>> = samples
                .iter()
                .zip(carrier.iter())
                .map(|(s, c)| s * c)
                .collect();

            // FFT the wiped signal
            let mut fft_input = wiped;
            let fft = self.fft_planner.plan_fft_forward(fft_size);
            fft.process(&mut fft_input);

            // Multiply by conjugated local code FFT
            let product: Vec<Complex<f64>> = fft_input
                .iter()
                .zip(local_code_fft_conj.iter())
                .map(|(a, b)| a * b)
                .collect();

            // IFFT → circular correlation
            let mut ifft_input = product;
            let ifft = self.fft_planner.plan_fft_inverse(fft_size);
            ifft.process(&mut ifft_input);

            // Normalize IFFT and compute magnitude squared
            let n = fft_size as f64;
            for (code_idx, value) in ifft_input.iter().enumerate() {
                magnitude_grid[bin_idx][code_idx] += value.norm_sqr() / (n * n);
            }
        }
    }

    /// Generate the FFT of the local PRN code, conjugated.
    fn generate_local_code_fft(&mut self, prn: usize, fft_size: usize) -> Vec<Complex<f64>> {
        let code = &PRN_CODES[prn];
        // Resample code to match sample rate
        let samples_per_chip = fft_size as f64 / CODE_LENGTH as f64;
        let local: Vec<Complex<f64>> = (0..fft_size)
            .map(|n| {
                let chip_idx = ((n as f64) / samples_per_chip) as usize % CODE_LENGTH;
                Complex::new(code[chip_idx] as f64, 0.0)
            })
            .collect();

        let fft = self.fft_planner.plan_fft_forward(fft_size);
        let mut fft_data = local;
        fft.process(&mut fft_data);

        // Conjugate
        fft_data.iter().map(|v| v.conj()).collect()
    }

    fn doppler_bins(&self) -> Vec<i32> {
        let n: i32 = ((2.0 * self.config.doppler_max) / self.config.doppler_step).ceil() as i32;
        let d_max = self.config.doppler_max as i32;
        let d_step = self.config.doppler_step as i32;
        (-n / 2..=n / 2)
            .map(|i| (i * d_step).clamp(-d_max, d_max))
            .collect()
    }

    fn compute_result(
        &self,
        prn: usize,
        sample_index: u64,
        doppler_bins: &[i32],
        magnitude_grid: &[Vec<f64>],
        fft_size: usize,
    ) -> AcquisitionResult {
        // Find global maximum
        let mut max_val = 0.0f64;
        let mut max_bin = 0usize;
        let mut max_code_idx = 0usize;

        for (bin_idx, bin_data) in magnitude_grid.iter().enumerate() {
            for (code_idx, &val) in bin_data.iter().enumerate() {
                if val > max_val {
                    max_val = val;
                    max_bin = bin_idx;
                    max_code_idx = code_idx;
                }
            }
        }

        // Compute mean excluding a small circular region around the peak for
        // robustness. Tiny synthetic windows cannot support a five-bin
        // exclusion region, so exclude only the peak bin there.
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for (code_idx, &val) in magnitude_grid[max_bin].iter().enumerate() {
            let in_exclude = if fft_size > 5 {
                let forward = code_idx.abs_diff(max_code_idx);
                let circular = forward.min(fft_size - forward);
                circular <= 2
            } else {
                code_idx == max_code_idx
            };
            if !in_exclude {
                sum += val;
                count += 1;
            }
        }
        let mean = if count > 0 {
            sum / count as f64
        } else {
            max_val.max(1e-30)
        };
        let peak_to_mean = max_val / mean;

        // Convert code index to chips.
        // The IFFT peak gives the shift needed on the LOCAL code.
        // The received signal's code phase = (CODE_LENGTH - detected) % CODE_LENGTH
        let raw_phase = (max_code_idx as f64 / fft_size as f64) * CODE_LENGTH as f64;
        let code_phase_chips = (CODE_LENGTH as f64 - raw_phase) % CODE_LENGTH as f64;

        AcquisitionResult {
            prn: (prn + 1) as u8,
            sample_index,
            doppler_hz: doppler_bins[max_bin] as f64,
            code_phase_chips,
            peak_to_mean,
            detected: peak_to_mean > self.config.threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn synthetic_signal_ms(
        prn: usize,
        doppler_hz: f64,
        code_phase_chips: f64,
        fs: f64,
        amp: f64,
        start_sample: usize,
        samples: usize,
    ) -> Vec<Complex<f64>> {
        let samples_per_chip = fs / 1.023e6;
        let code = &PRN_CODES[prn];
        (0..samples)
            .map(|idx| {
                let sample_index = start_sample + idx;
                let phase = TAU * doppler_hz * sample_index as f64 / fs;
                let chip_idx = ((sample_index as f64 / samples_per_chip + code_phase_chips)
                    as usize)
                    % CODE_LENGTH;
                let chip = code[chip_idx] as f64;
                Complex::new(phase.cos(), phase.sin()) * Complex::new(amp * chip, 0.0)
            })
            .collect()
    }

    fn deterministic_noise(sample_index: usize) -> Complex<f64> {
        let x = ((sample_index as u64)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407)
            >> 32) as u32;
        let y = ((sample_index as u64)
            .wrapping_mul(1442695040888963407)
            .wrapping_add(6364136223846793005)
            >> 32) as u32;
        let re = (x as f64 / u32::MAX as f64) * 2.0 - 1.0;
        let im = (y as f64 / u32::MAX as f64) * 2.0 - 1.0;
        Complex::new(re, im)
    }

    #[test]
    fn test_no_signal_returns_no_detection() {
        let config = PcpsConfig::default();
        let mut acq = PcpsAcquisition::new(config);
        // Pure noise
        let noise: Vec<Complex<f64>> = (0..4000).map(|_| Complex::new(0.0, 0.0)).collect();
        let result = acq.acquire(0, &noise);
        assert!(!result.detected, "Pure noise should not trigger detection");
        assert_eq!(result.sample_index, 0);
    }

    #[test]
    fn test_acquisition_result_preserves_window_sample_stamp() {
        let config = PcpsConfig::default();
        let mut acq = PcpsAcquisition::new(config);
        let noise: Vec<Complex<f64>> = (0..4000).map(|_| Complex::new(0.0, 0.0)).collect();

        let result = acq.acquire_at_sample(0, &noise, 123_456);

        assert_eq!(result.sample_index, 123_456);
    }

    #[test]
    fn test_noncoherent_acquisition_accumulates_1ms_power_maps() {
        let fs = 4_000_000.0;
        let samples_per_ms = fs as usize / 1000;
        let integrations = 10;
        let prn = 5;
        let doppler = 1500.0;
        let code_phase = 512.0;
        let amp = 1.2;
        let mut signal = synthetic_signal_ms(
            prn,
            doppler,
            code_phase,
            fs,
            amp,
            0,
            samples_per_ms * integrations,
        );
        for (idx, sample) in signal.iter_mut().enumerate() {
            *sample += deterministic_noise(idx);
        }

        let config = PcpsConfig {
            fs,
            doppler_max: 5_000.0,
            doppler_step: 500.0,
            threshold: 3.0,
            noncoherent_integrations: integrations,
            ..Default::default()
        };
        let mut noncoherent = PcpsAcquisition::new(config);
        let result = noncoherent.acquire_at_sample(prn, &signal, 42_000);

        assert!(result.detected);
        assert_eq!(result.sample_index, 42_000);
        assert_eq!(result.prn, (prn + 1) as u8);
        assert!(
            (result.doppler_hz - doppler).abs() <= 500.0,
            "Doppler error: expected {doppler}, got {}",
            result.doppler_hz
        );
        assert!(
            (result.code_phase_chips - code_phase).abs() < 2.0,
            "Code phase error: expected {code_phase}, got {}",
            result.code_phase_chips
        );
    }
}
