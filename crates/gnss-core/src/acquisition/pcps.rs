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
    /// `samples` must contain exactly `samples_per_code` complex samples
    /// (1 ms of data at the configured sample rate).
    pub fn acquire(&mut self, prn: usize, samples: &[Complex<f64>]) -> AcquisitionResult {
        let samples_per_code = (self.config.fs * self.config.coherent_integration_ms / 1000.0) as usize;
        debug_assert_eq!(samples.len(), samples_per_code,
            "Expected {} samples, got {}", samples_per_code, samples.len());

        let fft_size = samples_per_code;
        let doppler_bins = self.doppler_bins();

        // Step 1: Generate local code (resampled to fs) and compute FFT + conjugate
        let local_code_fft_conj = self.generate_local_code_fft(prn, fft_size);

        // Step 2: Allocate magnitude grid [doppler_bins × fft_size]
        let mut magnitude_grid = vec![vec![0.0f64; fft_size]; doppler_bins.len()];

        // Step 3: Doppler search loop (cf. GNSS-SDR doppler_grid)
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
            let wiped: Vec<Complex<f64>> = samples.iter()
                .zip(carrier.iter())
                .map(|(s, c)| s * c)
                .collect();

            // FFT the wiped signal
            let mut fft_input = wiped;
            let fft = self.fft_planner.plan_fft_forward(fft_size);
            fft.process(&mut fft_input);

            // Multiply by conjugated local code FFT
            let product: Vec<Complex<f64>> = fft_input.iter()
                .zip(local_code_fft_conj.iter())
                .map(|(a, b)| a * b)
                .collect();

            // IFFT → circular correlation
            let mut ifft_input = product;
            let ifft = self.fft_planner.plan_fft_inverse(fft_size);
            ifft.process(&mut ifft_input);

            // Normalize IFFT and compute magnitude squared
            let n = fft_size as f64;
            let mag_sq: Vec<f64> = ifft_input.iter()
                .map(|v| v.norm_sqr() / (n * n))
                .collect();
            magnitude_grid[bin_idx] = mag_sq;
        }

        // Step 4: Compute statistics (cf. GNSS-SDR compute_statistics)
        self.compute_result(prn, &doppler_bins, &magnitude_grid, fft_size)
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

        // Compute mean (excluding a small region around the peak for robustness)
        let exclude_start = if max_code_idx >= 2 { max_code_idx - 2 } else { fft_size + max_code_idx - 2 };
        let exclude_end = (max_code_idx + 3) % fft_size;
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for (code_idx, &val) in magnitude_grid[max_bin].iter().enumerate() {
            let in_exclude = if exclude_start <= exclude_end {
                code_idx >= exclude_start && code_idx < exclude_end
            } else {
                code_idx >= exclude_start || code_idx < exclude_end
            };
            if !in_exclude {
                sum += val;
                count += 1;
            }
        }
        let mean = if count > 0 { sum / count as f64 } else { 1.0 };
        let peak_to_mean = max_val / mean;

        // Convert code index to chips
        let code_phase_chips = (max_code_idx as f64 / fft_size as f64) * CODE_LENGTH as f64;

        AcquisitionResult {
            prn: (prn + 1) as u8,
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

    #[test]
    fn test_no_signal_returns_no_detection() {
        let config = PcpsConfig::default();
        let mut acq = PcpsAcquisition::new(config);
        // Pure noise
        let noise: Vec<Complex<f64>> = (0..4000)
            .map(|_| Complex::new(0.0, 0.0))
            .collect();
        let result = acq.acquire(0, &noise);
        assert!(!result.detected, "Pure noise should not trigger detection");
    }
}
