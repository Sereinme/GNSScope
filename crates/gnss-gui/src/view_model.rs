use gnss_core::pvt::{
    PvtCandidateDisposition, PvtCandidateOutput, PvtCandidateStatusOutput, PvtEpochOutput,
    PvtEpochSolutionKind, PvtQualityOutput, PvtSatelliteOutput, PvtVelocityOutput, UtcDateTime,
};

/// Positioning fix dimensionality of the selected epoch solution, derived
/// (never fabricated) from existing receiver outputs: the GPS L1 C/A solver
/// estimates the four-unknown state `[x, y, z, clock]`
/// (`gnss-core/src/pvt/solver.rs`), so a converged, non-`Unconverged`
/// solution that used at least four satellites and carries a finite position
/// is a genuine 3D fix. Anything else derives `None` and renders as the
/// honest "—/不可用" fallback (spec §3.11 / issue 17); the candidate kind
/// (`Raw/Smoothed/Kalman`) is never presented as a fix type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiFixType {
    Fix3D,
}

impl GuiFixType {
    /// UI label (§3.0 example: "解类型 3D Fix"; the technical label stays
    /// English in both locales, like other measurement abbreviations).
    pub fn label(self) -> &'static str {
        match self {
            Self::Fix3D => "3D Fix",
        }
    }
}

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
    pub candidates: Vec<GuiPvtCandidate>,
    pub candidate_statuses: Vec<GuiPvtCandidateStatus>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiPvtCandidate {
    pub solution_kind: PvtEpochSolutionKind,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub ecef_m: [f64; 3],
    pub receiver_clock_bias_us: f64,
    pub velocity: Option<GuiPvtVelocity>,
    pub quality: GuiPvtQuality,
    pub satellites: Vec<GuiSatelliteGeometry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuiPvtCandidateStatus {
    pub solution_kind: PvtEpochSolutionKind,
    pub disposition: PvtCandidateDisposition,
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

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct GuiPvtQuality {
    pub converged: bool,
    pub n_sats: usize,
    pub used_prns: Vec<u8>,
    pub excluded_prns: Vec<u8>,
    pub residual_rms_m: f64,
    pub standardized_residual_rms: f64,
    pub gdop: f64,
    pub hdop: f64,
    /// Position dilution of precision (issue 19). `None` when the receiver
    /// did not compute a genuine value (see [`dop_metric`]); renders as the
    /// honest "—" — never 0, never a recomputed estimate.
    pub pdop: Option<f64>,
    /// Vertical dilution of precision (issue 19); same honesty rule as
    /// [`Self::pdop`].
    pub vdop: Option<f64>,
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
            velocity: output.velocity.as_ref().map(GuiPvtVelocity::from_output),
            quality: GuiPvtQuality::from_output(&output.quality),
            satellites: output
                .satellites
                .iter()
                .map(GuiSatelliteGeometry::from_output)
                .collect(),
            candidates: output
                .candidates
                .iter()
                .map(GuiPvtCandidate::from_output)
                .collect(),
            candidate_statuses: output
                .candidate_statuses
                .iter()
                .map(GuiPvtCandidateStatus::from_output)
                .collect(),
        }
    }

    pub fn from_epoch_outputs(outputs: &[PvtEpochOutput]) -> Vec<Self> {
        outputs.iter().map(Self::from_epoch_output).collect()
    }

    /// Real fix type of the selected epoch solution, derived from existing
    /// outputs only. `None` when the epoch has no valid position fix
    /// (unconverged, fewer than four used satellites, or a non-finite
    /// position) — callers must render the honest "—/不可用" fallback and
    /// must not substitute the candidate kind Debug string (issue 17).
    pub fn fix_type(&self) -> Option<GuiFixType> {
        let position_finite = self.latitude_deg.is_finite()
            && self.longitude_deg.is_finite()
            && self.altitude_m.is_finite()
            && self.ecef_m.iter().all(|value| value.is_finite());
        (self.solution_kind != PvtEpochSolutionKind::Unconverged
            && self.quality.converged
            && self.quality.n_sats >= 4
            && position_finite)
            .then_some(GuiFixType::Fix3D)
    }

    pub fn candidate(&self, kind: PvtEpochSolutionKind) -> Option<&GuiPvtCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.solution_kind == kind)
    }

    pub fn candidate_disposition(&self, kind: PvtEpochSolutionKind) -> PvtCandidateDisposition {
        self.candidate_statuses
            .iter()
            .find(|status| status.solution_kind == kind)
            .map(|status| status.disposition)
            .unwrap_or_else(|| {
                if self.solution_kind == kind {
                    PvtCandidateDisposition::Selected
                } else if let Some(candidate) = self.candidate(kind) {
                    if candidate.quality.converged {
                        PvtCandidateDisposition::Available
                    } else {
                        PvtCandidateDisposition::NonConverged
                    }
                } else {
                    PvtCandidateDisposition::Unavailable
                }
            })
    }
}

impl GuiPvtCandidateStatus {
    fn from_output(output: &PvtCandidateStatusOutput) -> Self {
        Self {
            solution_kind: output.solution_kind,
            disposition: output.disposition,
        }
    }
}

impl GuiPvtCandidate {
    fn from_output(output: &PvtCandidateOutput) -> Self {
        Self {
            solution_kind: output.solution_kind,
            latitude_deg: output.lla_rad[0].to_degrees(),
            longitude_deg: output.lla_rad[1].to_degrees(),
            altitude_m: output.lla_rad[2],
            ecef_m: output.ecef,
            receiver_clock_bias_us: output.receiver_clock_bias_s * 1e6,
            velocity: output.velocity.as_ref().map(GuiPvtVelocity::from_output),
            quality: GuiPvtQuality::from_output(&output.quality),
            satellites: output
                .satellites
                .iter()
                .map(GuiSatelliteGeometry::from_output)
                .collect(),
        }
    }
}

impl GuiPvtVelocity {
    fn from_output(output: &PvtVelocityOutput) -> Self {
        Self {
            east_mps: output.enu_velocity_mps[0],
            north_mps: output.enu_velocity_mps[1],
            up_mps: output.enu_velocity_mps[2],
            speed_mps: output.speed_mps,
            horizontal_speed_mps: output.horizontal_speed_mps,
            vertical_speed_mps: output.vertical_speed_mps,
            heading_deg: output.heading_rad.map(f64::to_degrees),
            clock_drift_mps: output.clock_drift_mps,
            residual_rms_mps: output.residual_rms_mps,
        }
    }
}

impl GuiPvtQuality {
    fn from_output(output: &PvtQualityOutput) -> Self {
        Self {
            converged: output.converged,
            n_sats: output.n_sats,
            used_prns: output.used_prns.clone(),
            excluded_prns: output.excluded_prns.clone(),
            residual_rms_m: output.residual_rms_m,
            standardized_residual_rms: output.standardized_residual_rms,
            gdop: output.gdop,
            hdop: output.hdop,
            pdop: dop_metric(output.pdop),
            vdop: dop_metric(output.vdop),
            horizontal_sigma_m: output.horizontal_sigma_m,
            vertical_sigma_m: output.vertical_sigma_m,
            velocity_sigma_mps: output.velocity_sigma_mps,
        }
    }
}

/// Map a core DOP metric onto the honest GUI option (issue 19). `gnss-core`
/// reports PDOP/VDOP as a plain `f64` whose `0.0` default means "not
/// computed" (see `PvtSolution::default` in `gnss-core/src/pvt/solver.rs`);
/// a physical DOP is always ≥ 1, so only a finite positive value is carried.
/// Anything else maps to `None` and renders as "—" — never 0, never a
/// recomputed estimate (spec §3.8 / ADR-0003 run immutability).
fn dop_metric(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

impl GuiSatelliteGeometry {
    fn from_output(output: &PvtSatelliteOutput) -> Self {
        Self {
            prn: output.prn,
            azimuth_deg: output.azimuth_rad.to_degrees(),
            elevation_deg: output.elevation_rad.to_degrees(),
            residual_m: output.residual_m,
            standardized_residual: output.standardized_residual,
            pseudorange_sigma_m: output.pseudorange_sigma_m,
            used_in_solution: output.used_in_solution,
            raim_excluded: output.raim_excluded,
        }
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
            candidates: Vec::new(),
            candidate_statuses: Vec::new(),
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
        // PDOP/VDOP cross the output seam as honest options (issue 19).
        assert_eq!(view.quality.pdop, Some(3.0));
        assert_eq!(view.quality.vdop, Some(2.4));
        assert_eq!(view.satellites.len(), 1);
        assert_eq!(view.satellites[0].prn, 20);
        assert!((view.satellites[0].azimuth_deg - 1.25_f64.to_degrees()).abs() < 1e-12);
        assert!((view.satellites[0].elevation_deg - 0.75_f64.to_degrees()).abs() < 1e-12);
    }

    #[test]
    fn fix_type_is_derived_only_from_a_real_3d_solution() {
        // The sample fixture is a converged Kalman solution with five used
        // satellites and a finite position, so it derives a genuine 3D fix.
        let healthy = crate::sample_pvt_view_model();
        assert_eq!(healthy.fix_type(), Some(GuiFixType::Fix3D));
        assert_eq!(healthy.fix_type().expect("3D fix").label(), "3D Fix");

        // An unconverged selection never yields a fix type.
        let mut unconverged = crate::sample_pvt_view_model();
        unconverged.solution_kind = PvtEpochSolutionKind::Unconverged;
        assert_eq!(unconverged.fix_type(), None);

        // A non-converged quality flag never yields a fix type.
        let mut not_converged = crate::sample_pvt_view_model();
        not_converged.quality.converged = false;
        assert_eq!(not_converged.fix_type(), None);

        // Fewer than four used satellites cannot resolve [x, y, z, clock].
        let mut undersampled = crate::sample_pvt_view_model();
        undersampled.quality.n_sats = 3;
        assert_eq!(undersampled.fix_type(), None);

        // A non-finite position is not a fix, even when everything else holds.
        let mut invalid_position = crate::sample_pvt_view_model();
        invalid_position.latitude_deg = f64::NAN;
        assert_eq!(invalid_position.fix_type(), None);
    }

    #[test]
    fn from_output_maps_uncomputed_dop_metrics_to_none_and_never_zero() {
        // gnss-core reports PDOP/VDOP as plain `f64`; the 0.0 default of
        // `PvtSolution` (and any non-finite value) means "not computed" and
        // must surface as `None` — never `Some(0.0)` (issue 19 honesty rule).
        let output = PvtQualityOutput {
            converged: false,
            n_sats: 0,
            used_prns: Vec::new(),
            excluded_prns: Vec::new(),
            residual_rms_m: 0.0,
            standardized_residual_rms: 0.0,
            max_standardized_residual: 0.0,
            epoch_quality_scale: 1.0,
            hdop: 0.0,
            vdop: 0.0,
            pdop: 0.0,
            gdop: 0.0,
            horizontal_sigma_m: 0.0,
            vertical_sigma_m: 0.0,
            position_sigma_m: 0.0,
            velocity_sigma_mps: None,
        };
        let quality = GuiPvtQuality::from_output(&output);
        assert_eq!(quality.pdop, None);
        assert_eq!(quality.vdop, None);

        let mut non_finite = output;
        non_finite.pdop = f64::NAN;
        non_finite.vdop = f64::INFINITY;
        let quality = GuiPvtQuality::from_output(&non_finite);
        assert_eq!(quality.pdop, None);
        assert_eq!(quality.vdop, None);

        // Genuine computed values pass through unchanged.
        assert_eq!(dop_metric(3.12), Some(3.12));
        assert_eq!(dop_metric(1.0), Some(1.0));
        assert_eq!(dop_metric(0.0), None);
        assert_eq!(dop_metric(-2.5), None);
    }
}
