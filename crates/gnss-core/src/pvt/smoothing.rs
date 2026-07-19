//! Carrier-smoothed pseudorange filters.

use crate::pvt::pseudorange::Pseudorange;
use std::collections::HashMap;

const C_LIGHT: f64 = 299_792_458.0;
const GPS_L1_HZ: f64 = 1_575_420_000.0;

/// Hatch filter configuration for carrier-smoothed code pseudorange.
#[derive(Debug, Clone)]
pub struct HatchConfig {
    /// Maximum smoothing window length, in epochs.
    pub window_epochs: usize,
    /// Reset a PRN state when receive-time gap exceeds this value.
    pub max_gap_s: f64,
    /// Reset when code-carrier divergence exceeds this threshold.
    pub divergence_reset_threshold_m: f64,
    /// Reject smoothed WLS output when its residual RMS exceeds raw by this ratio.
    pub max_smoothed_residual_rms_ratio: f64,
    /// Additive residual RMS margin for smoothed WLS acceptance [m].
    pub max_smoothed_residual_rms_margin_m: f64,
    /// Reset all smoothing states when a smoothed WLS candidate is rejected.
    pub reset_on_rejected_solution: bool,
}

impl Default for HatchConfig {
    fn default() -> Self {
        Self {
            window_epochs: 100,
            max_gap_s: 2.0,
            divergence_reset_threshold_m: 100.0,
            max_smoothed_residual_rms_ratio: 1.0,
            max_smoothed_residual_rms_margin_m: 2.0,
            reset_on_rejected_solution: false,
        }
    }
}

#[derive(Debug, Clone)]
struct HatchState {
    smoothed_pseudorange_m: f64,
    last_receive_time_sow: f64,
    last_doppler_hz: f64,
    count: usize,
}

/// Per-PRN Hatch carrier smoother.
#[derive(Debug, Clone)]
pub struct HatchSmoother {
    config: HatchConfig,
    states: HashMap<u8, HatchState>,
}

impl HatchSmoother {
    pub fn new(config: HatchConfig) -> Self {
        Self {
            config,
            states: HashMap::new(),
        }
    }

    /// Smooth all pseudoranges in one epoch.
    ///
    /// Doppler is integrated with the common GNSS sign convention
    /// `range_rate = -lambda * doppler`.
    pub fn smooth_epoch(&mut self, pseudoranges: &[Pseudorange]) -> Vec<Pseudorange> {
        pseudoranges
            .iter()
            .map(|pr| {
                let smoothed_m = self.smooth_one(pr);
                let mut out = *pr;
                out.pseudorange_m = smoothed_m;
                out
            })
            .collect()
    }

    pub fn reset_prn(&mut self, prn: u8) {
        self.states.remove(&prn);
    }

    pub fn reset(&mut self) {
        self.states.clear();
    }

    pub fn config(&self) -> &HatchConfig {
        &self.config
    }

    fn smooth_one(&mut self, pr: &Pseudorange) -> f64 {
        let Some(prev) = self.states.get(&pr.prn).cloned() else {
            self.states.insert(pr.prn, initial_state(pr));
            return pr.pseudorange_m;
        };

        let dt = pr.receive_time_sow - prev.last_receive_time_sow;
        if dt <= 0.0 || dt > self.config.max_gap_s || !dt.is_finite() {
            self.states.insert(pr.prn, initial_state(pr));
            return pr.pseudorange_m;
        }

        let lambda_m = C_LIGHT / GPS_L1_HZ;
        let doppler_avg_hz = 0.5 * (prev.last_doppler_hz + pr.doppler_hz);
        let carrier_delta_m = -lambda_m * doppler_avg_hz * dt;
        let predicted = prev.smoothed_pseudorange_m + carrier_delta_m;
        let divergence_m = pr.pseudorange_m - predicted;
        if divergence_m.abs() > self.config.divergence_reset_threshold_m {
            self.states.insert(pr.prn, initial_state(pr));
            return pr.pseudorange_m;
        }

        let count = (prev.count + 1).min(self.config.window_epochs.max(1));
        let alpha = 1.0 / count as f64;
        let smoothed = alpha * pr.pseudorange_m + (1.0 - alpha) * predicted;
        self.states.insert(
            pr.prn,
            HatchState {
                smoothed_pseudorange_m: smoothed,
                last_receive_time_sow: pr.receive_time_sow,
                last_doppler_hz: pr.doppler_hz,
                count,
            },
        );
        smoothed
    }
}

fn initial_state(pr: &Pseudorange) -> HatchState {
    HatchState {
        smoothed_pseudorange_m: pr.pseudorange_m,
        last_receive_time_sow: pr.receive_time_sow,
        last_doppler_hz: pr.doppler_hz,
        count: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hatch_smoother_follows_doppler_integrated_range() {
        let mut smoother = HatchSmoother::new(HatchConfig {
            window_epochs: 10,
            max_gap_s: 2.0,
            divergence_reset_threshold_m: 1_000.0,
            ..Default::default()
        });
        let first = pseudorange(1, 20_000_000.0, 100.0, -1_000.0);
        let second = pseudorange(1, 20_000_190.0, 101.0, -1_000.0);

        let out0 = smoother.smooth_epoch(&[first]);
        let out1 = smoother.smooth_epoch(&[second]);

        assert_eq!(out0[0].pseudorange_m, first.pseudorange_m);
        let lambda_m = C_LIGHT / GPS_L1_HZ;
        let predicted = first.pseudorange_m + lambda_m * 1_000.0;
        let expected = 0.5 * second.pseudorange_m + 0.5 * predicted;
        assert!((out1[0].pseudorange_m - expected).abs() < 1e-6);
    }

    #[test]
    fn test_hatch_smoother_keeps_prn_states_independent() {
        let mut smoother = HatchSmoother::new(HatchConfig::default());
        let out0 = smoother.smooth_epoch(&[
            pseudorange(1, 10_000.0, 10.0, 0.0),
            pseudorange(2, 20_000.0, 10.0, 0.0),
        ]);
        let out1 = smoother.smooth_epoch(&[
            pseudorange(1, 10_002.0, 11.0, 0.0),
            pseudorange(2, 20_006.0, 11.0, 0.0),
        ]);

        assert_eq!(out0[0].pseudorange_m, 10_000.0);
        assert_eq!(out0[1].pseudorange_m, 20_000.0);
        assert_eq!(out1[0].pseudorange_m, 10_001.0);
        assert_eq!(out1[1].pseudorange_m, 20_003.0);
    }

    #[test]
    fn test_hatch_smoother_resets_on_gap_or_divergence() {
        let mut smoother = HatchSmoother::new(HatchConfig {
            window_epochs: 10,
            max_gap_s: 1.0,
            divergence_reset_threshold_m: 10.0,
            ..Default::default()
        });
        smoother.smooth_epoch(&[pseudorange(1, 10_000.0, 10.0, 0.0)]);
        let gap = smoother.smooth_epoch(&[pseudorange(1, 10_050.0, 12.0, 0.0)]);
        assert_eq!(gap[0].pseudorange_m, 10_050.0);

        let normal = smoother.smooth_epoch(&[pseudorange(1, 10_052.0, 12.5, 0.0)]);
        assert_eq!(normal[0].pseudorange_m, 10_051.0);
        let divergent = smoother.smooth_epoch(&[pseudorange(1, 10_200.0, 13.0, 0.0)]);
        assert_eq!(divergent[0].pseudorange_m, 10_200.0);
    }

    fn pseudorange(
        prn: u8,
        pseudorange_m: f64,
        receive_time_sow: f64,
        doppler_hz: f64,
    ) -> Pseudorange {
        Pseudorange {
            prn,
            pseudorange_m,
            transmit_time_sow: receive_time_sow - 0.075,
            receive_time_sow,
            cn0_db_hz: 45.0,
            doppler_hz,
            tracking_jitter_chips: 0.0,
        }
    }
}
