use gnss_core::pvt::{PvtEpochOutput, PvtEpochSolutionKind, UtcDateTime};

#[derive(Debug, Clone)]
pub struct GuiPvtViewModel {
    pub epoch_index: usize,
    pub solution_kind: PvtEpochSolutionKind,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub ecef_m: [f64; 3],
    pub time: GuiPvtTime,
    pub velocity: Option<GuiPvtVelocity>,
    pub quality: GuiPvtQuality,
    pub satellites: Vec<GuiSatelliteGeometry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiSatelliteGeometry {
    pub prn: u8,
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub residual_m: f64,
    pub standardized_residual: f64,
    pub pseudorange_sigma_m: f64,
    pub used_in_solution: bool,
    pub raim_excluded: bool,
}

#[derive(Debug, Clone)]
pub struct GuiPvtTime {
    pub gps_week: Option<u16>,
    pub receiver_time_sow: Option<f64>,
    pub corrected_gps_time_sow: Option<f64>,
    pub utc_unix_seconds: Option<f64>,
    pub utc: Option<UtcDateTime>,
    pub receiver_clock_bias_us: f64,
    pub receiver_clock_drift_ppm: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct GuiPvtVelocity {
    pub east_mps: f64,
    pub north_mps: f64,
    pub up_mps: f64,
    pub speed_mps: f64,
    pub horizontal_speed_mps: f64,
    pub vertical_speed_mps: f64,
    pub heading_deg: Option<f64>,
    pub clock_drift_mps: f64,
    pub residual_rms_mps: f64,
}

#[derive(Debug, Clone)]
pub struct GuiPvtQuality {
    pub converged: bool,
    pub n_sats: usize,
    pub used_prns: Vec<u8>,
    pub excluded_prns: Vec<u8>,
    pub residual_rms_m: f64,
    pub standardized_residual_rms: f64,
    pub gdop: f64,
    pub hdop: f64,
    pub horizontal_sigma_m: f64,
    pub vertical_sigma_m: f64,
    pub velocity_sigma_mps: Option<f64>,
}

impl GuiPvtViewModel {
    pub fn from_epoch_output(output: &PvtEpochOutput) -> Self {
        Self {
            epoch_index: output.epoch_index,
            solution_kind: output.solution_kind,
            latitude_deg: output.lla_rad[0].to_degrees(),
            longitude_deg: output.lla_rad[1].to_degrees(),
            altitude_m: output.lla_rad[2],
            ecef_m: output.ecef,
            time: GuiPvtTime {
                gps_week: output.time.gps_week,
                receiver_time_sow: output.time.receiver_time_sow,
                corrected_gps_time_sow: output.time.corrected_gps_time_sow,
                utc_unix_seconds: output.time.utc_unix_seconds,
                utc: output.time.utc,
                receiver_clock_bias_us: output.time.receiver_clock_bias_s * 1e6,
                receiver_clock_drift_ppm: output
                    .time
                    .receiver_clock_drift_sps
                    .map(|clock_drift_sps| clock_drift_sps * 1e6),
            },
            velocity: output.velocity.as_ref().map(|velocity| GuiPvtVelocity {
                east_mps: velocity.enu_velocity_mps[0],
                north_mps: velocity.enu_velocity_mps[1],
                up_mps: velocity.enu_velocity_mps[2],
                speed_mps: velocity.speed_mps,
                horizontal_speed_mps: velocity.horizontal_speed_mps,
                vertical_speed_mps: velocity.vertical_speed_mps,
                heading_deg: velocity.heading_rad.map(|heading| heading.to_degrees()),
                clock_drift_mps: velocity.clock_drift_mps,
                residual_rms_mps: velocity.residual_rms_mps,
            }),
            quality: GuiPvtQuality {
                converged: output.quality.converged,
                n_sats: output.quality.n_sats,
                used_prns: output.quality.used_prns.clone(),
                excluded_prns: output.quality.excluded_prns.clone(),
                residual_rms_m: output.quality.residual_rms_m,
                standardized_residual_rms: output.quality.standardized_residual_rms,
                gdop: output.quality.gdop,
                hdop: output.quality.hdop,
                horizontal_sigma_m: output.quality.horizontal_sigma_m,
                vertical_sigma_m: output.quality.vertical_sigma_m,
                velocity_sigma_mps: output.quality.velocity_sigma_mps,
            },
            satellites: output
                .satellites
                .iter()
                .map(|satellite| GuiSatelliteGeometry {
                    prn: satellite.prn,
                    azimuth_deg: satellite.azimuth_rad.to_degrees(),
                    elevation_deg: satellite.elevation_rad.to_degrees(),
                    residual_m: satellite.residual_m,
                    standardized_residual: satellite.standardized_residual,
                    pseudorange_sigma_m: satellite.pseudorange_sigma_m,
                    used_in_solution: satellite.used_in_solution,
                    raim_excluded: satellite.raim_excluded,
                })
                .collect(),
        }
    }

    pub fn from_epoch_outputs(outputs: &[PvtEpochOutput]) -> Vec<Self> {
        outputs.iter().map(Self::from_epoch_output).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gnss_core::pvt::{PvtQualityOutput, PvtSatelliteOutput, PvtTimeOutput, PvtVelocityOutput};

    #[test]
    fn test_view_model_exposes_position_velocity_time_quality() {
        let output = PvtEpochOutput {
            epoch_index: 7,
            solution_kind: PvtEpochSolutionKind::Kalman,
            time: PvtTimeOutput {
                gps_week: Some(1734),
                receiver_time_sow: Some(368_658.25),
                corrected_gps_time_sow: Some(368_658.249_9),
                utc_unix_seconds: Some(1_365_056_642.249_9),
                utc: Some(UtcDateTime {
                    year: 2013,
                    month: 4,
                    day: 4,
                    hour: 6,
                    minute: 24,
                    second: 2.249_9,
                }),
                transmit_time_sow_min: Some(368_658.17),
                transmit_time_sow_max: Some(368_658.18),
                receiver_clock_bias_s: 1.5e-6,
                receiver_clock_drift_sps: Some(2e-8),
            },
            ecef: [1.0, 2.0, 3.0],
            lla_rad: [41.0_f64.to_radians(), 2.0_f64.to_radians(), 70.0],
            velocity: Some(PvtVelocityOutput {
                ecef_velocity_mps: [0.0, 0.0, 0.0],
                enu_velocity_mps: [3.0, 4.0, -0.5],
                speed_mps: 5.025,
                horizontal_speed_mps: 5.0,
                vertical_speed_mps: -0.5,
                heading_rad: Some(3.0_f64.atan2(4.0)),
                clock_drift_mps: 6.0,
                clock_drift_sps: 2e-8,
                residual_rms_mps: 0.2,
                used_prns: vec![1, 3, 8, 11],
            }),
            quality: PvtQualityOutput {
                converged: true,
                n_sats: 5,
                used_prns: vec![1, 3, 8, 11, 20],
                excluded_prns: vec![17],
                residual_rms_m: 1.2,
                standardized_residual_rms: 0.7,
                max_standardized_residual: 1.5,
                epoch_quality_scale: 1.0,
                hdop: 1.8,
                vdop: 2.4,
                pdop: 3.0,
                gdop: 3.5,
                horizontal_sigma_m: 8.0,
                vertical_sigma_m: 12.0,
                position_sigma_m: 10.0,
                velocity_sigma_mps: Some(0.3),
            },
            satellites: vec![PvtSatelliteOutput {
                prn: 20,
                azimuth_rad: 1.25,
                elevation_rad: 0.75,
                residual_m: -0.4,
                standardized_residual: -0.2,
                pseudorange_sigma_m: 2.0,
                used_in_solution: true,
                raim_excluded: false,
            }],
        };

        let view = GuiPvtViewModel::from_epoch_output(&output);

        assert_eq!(view.epoch_index, 7);
        assert!((view.latitude_deg - 41.0).abs() < 1e-12);
        assert!((view.longitude_deg - 2.0).abs() < 1e-12);
        assert_eq!(view.time.gps_week, Some(1734));
        assert_eq!(view.time.receiver_clock_bias_us, 1.5);
        assert_eq!(view.time.utc.map(|utc| utc.year), Some(2013));
        assert_eq!(view.time.receiver_clock_drift_ppm, Some(0.02));
        let velocity = view.velocity.as_ref().unwrap();
        assert_eq!(velocity.horizontal_speed_mps, 5.0);
        assert_eq!(velocity.vertical_speed_mps, -0.5);
        assert_eq!(view.quality.n_sats, 5);
        assert_eq!(view.quality.excluded_prns, vec![17]);
        assert_eq!(view.satellites.len(), 1);
        assert_eq!(view.satellites[0].prn, 20);
        assert!((view.satellites[0].azimuth_deg - 1.25_f64.to_degrees()).abs() < 1e-12);
        assert!((view.satellites[0].elevation_deg - 0.75_f64.to_degrees()).abs() < 1e-12);
    }
}
