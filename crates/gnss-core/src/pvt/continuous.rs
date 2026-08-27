//! Continuous PVT epoch runner.

use crate::nav::ephemeris::{
    ephemeris_map_from_catalog_at_time, EphemerisCatalog, EphemerisMap, EphemerisSelectionConfig,
    IonoParams,
};
use crate::pvt::coords::{ecef_to_lla, lla_to_ecef, Ecef};
use crate::pvt::kalman::{KalmanConfig, KalmanSolution, PositionVelocityKalman};
use crate::pvt::pseudorange::{
    from_tracking_timestamp_common_receive_epoch, resolve_integer_ms_ambiguities,
    ObservablesBuilder, ObservablesInputSet, Pseudorange, TrackingObservation,
    TrackingTimestampObservation,
};
use crate::pvt::satpos::satellite_position;
use crate::pvt::smoothing::{HatchConfig, HatchSmoother};
use crate::pvt::solver::{solve_wls_with_config, PvtSolution, WlsConfig};
use crate::pvt::time::{gps_time_to_utc, normalize_gps_time, UtcDateTime};
use crate::pvt::velocity::{solve_doppler_velocity, VelocitySolution};
use std::collections::BTreeMap;

const C_LIGHT: f64 = 299_792_458.0;
const OMEGA_E: f64 = 7.292_115_146_7e-5;

#[derive(Debug, Clone)]
pub struct PseudorangeEpoch {
    pub pseudoranges: Vec<Pseudorange>,
}

impl PseudorangeEpoch {
    pub fn from_observables_input(
        input: &ObservablesInputSet<'_>,
        common_rx_sample: f64,
        fs: f64,
        nominal_anchor_travel_time_s: f64,
    ) -> Option<Self> {
        let observables = ObservablesBuilder::new(common_rx_sample, fs)
            .with_nominal_anchor_travel_time_s(nominal_anchor_travel_time_s)
            .with_tracking_timestamp_observations(&input.tracking_timestamp)
            .with_nco_reference_observations(&input.nco_reference)
            .build();
        Self::from_pseudoranges(observables.pseudoranges)
    }

    pub fn from_tracking_timestamp_observations(
        observations: &[TrackingTimestampObservation<'_>],
        common_rx_sample: f64,
        fs: f64,
        nominal_anchor_travel_time_s: f64,
    ) -> Option<Self> {
        Self::from_pseudoranges(from_tracking_timestamp_common_receive_epoch(
            observations,
            common_rx_sample,
            fs,
            nominal_anchor_travel_time_s,
        ))
    }

    pub fn from_tracking_observations(
        observations: &[TrackingObservation],
        common_rx_sample: f64,
        fs: f64,
        nominal_anchor_travel_time_s: f64,
    ) -> Option<Self> {
        let observables = ObservablesBuilder::new(common_rx_sample, fs)
            .with_nominal_anchor_travel_time_s(nominal_anchor_travel_time_s)
            .with_nco_reference_observations(observations)
            .build();
        Self::from_pseudoranges(observables.pseudoranges)
    }

    fn from_pseudoranges(pseudoranges: Vec<Pseudorange>) -> Option<Self> {
        (!pseudoranges.is_empty()).then_some(Self { pseudoranges })
    }
}

#[derive(Debug, Clone)]
pub struct IntegerMsAmbiguityConfig {
    pub max_shift_ms: i32,
}

impl Default for IntegerMsAmbiguityConfig {
    fn default() -> Self {
        Self { max_shift_ms: 2 }
    }
}

#[derive(Debug, Clone)]
pub struct VelocityQualityConfig {
    /// Reject Doppler velocity solutions with post-fit residual RMS above this value [m/s].
    pub max_residual_rms_mps: f64,
    /// Reject Doppler velocity solutions with receiver clock drift above this value [m/s].
    pub max_clock_drift_mps: f64,
    /// Optional receiver speed cap [m/s]. Use for known static or low-dynamics datasets.
    pub max_speed_mps: f64,
    /// Optional zero-velocity fallback sigma for Kalman when a Doppler solution is rejected [m/s].
    pub rejected_velocity_fallback_sigma_mps: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PositionQualityConfig {
    /// Minimum plausible receiver altitude for the terrestrial receiver profile [m].
    pub min_altitude_m: f64,
    /// Maximum plausible receiver altitude for the terrestrial receiver profile [m].
    pub max_altitude_m: f64,
    /// Reject position solutions with post-fit pseudorange residual RMS above this value [m].
    pub max_residual_rms_m: f64,
    /// Maximum implied receiver speed between accepted epochs [m/s].
    pub max_implied_speed_mps: f64,
    /// Fixed position-jump allowance for measurement noise and reacquisition [m].
    pub jump_margin_m: f64,
}

impl Default for PositionQualityConfig {
    fn default() -> Self {
        Self {
            min_altitude_m: -2_000.0,
            max_altitude_m: 20_000.0,
            max_residual_rms_m: 1_000.0,
            max_implied_speed_mps: 12_000.0,
            jump_margin_m: 1_000.0,
        }
    }
}

impl Default for VelocityQualityConfig {
    fn default() -> Self {
        Self {
            max_residual_rms_mps: 2.0,
            max_clock_drift_mps: 5_000.0,
            max_speed_mps: f64::INFINITY,
            rejected_velocity_fallback_sigma_mps: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContinuousPvtConfig {
    pub hatch: Option<HatchConfig>,
    pub integer_ms_ambiguity: Option<IntegerMsAmbiguityConfig>,
    /// Optional coarse receiver position used only to bootstrap cold-start WLS
    /// and integer-millisecond ambiguity resolution [lat rad, lon rad, alt m].
    pub initial_position_lla_rad: Option<[f64; 3]>,
    pub ephemeris_selection: EphemerisSelectionConfig,
    pub kalman: Option<KalmanConfig>,
    pub velocity_quality: VelocityQualityConfig,
    pub position_quality: PositionQualityConfig,
    pub wls: WlsConfig,
    /// Approximate full GPS week used to expand a broadcast 10-bit week number.
    pub gps_week_rollover_reference: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct ContinuousPvtEpochResult {
    pub epoch_index: usize,
    pub gps_week: Option<u16>,
    pub raw_pseudoranges: Vec<Pseudorange>,
    pub smoothed_pseudoranges: Option<Vec<Pseudorange>>,
    pub raw_solution: PvtSolution,
    pub raw_candidate_disposition: PvtCandidateDisposition,
    pub raw_velocity_solution: Option<VelocitySolution>,
    pub smoothed_solution: Option<PvtSolution>,
    pub smoothed_candidate_solution: Option<PvtSolution>,
    pub smoothed_candidate_disposition: PvtCandidateDisposition,
    pub smoothed_velocity_solution: Option<VelocitySolution>,
    pub kalman_solution: Option<KalmanSolution>,
    pub kalman_candidate_disposition: PvtCandidateDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PvtEpochSolutionKind {
    Raw,
    Smoothed,
    Kalman,
    Unconverged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PvtCandidateDisposition {
    Selected,
    Available,
    NonConverged,
    QualityRejected,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PvtTimeOutput {
    pub gps_week: Option<u16>,
    pub receiver_time_sow: Option<f64>,
    pub corrected_gps_time_sow: Option<f64>,
    pub utc_unix_seconds: Option<f64>,
    pub utc: Option<UtcDateTime>,
    pub transmit_time_sow_min: Option<f64>,
    pub transmit_time_sow_max: Option<f64>,
    pub receiver_clock_bias_s: f64,
    pub receiver_clock_drift_sps: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PvtVelocityOutput {
    pub ecef_velocity_mps: Ecef,
    pub enu_velocity_mps: Ecef,
    pub speed_mps: f64,
    pub horizontal_speed_mps: f64,
    pub vertical_speed_mps: f64,
    pub heading_rad: Option<f64>,
    pub clock_drift_mps: f64,
    pub clock_drift_sps: f64,
    pub residual_rms_mps: f64,
    pub used_prns: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PvtQualityOutput {
    pub converged: bool,
    pub n_sats: usize,
    pub used_prns: Vec<u8>,
    pub excluded_prns: Vec<u8>,
    pub residual_rms_m: f64,
    pub standardized_residual_rms: f64,
    pub max_standardized_residual: f64,
    pub epoch_quality_scale: f64,
    pub hdop: f64,
    pub vdop: f64,
    pub pdop: f64,
    pub gdop: f64,
    pub horizontal_sigma_m: f64,
    pub vertical_sigma_m: f64,
    pub position_sigma_m: f64,
    pub velocity_sigma_mps: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PvtSatelliteOutput {
    pub prn: u8,
    pub azimuth_rad: f64,
    pub elevation_rad: f64,
    pub residual_m: f64,
    pub standardized_residual: f64,
    pub pseudorange_sigma_m: f64,
    pub used_in_solution: bool,
    pub raim_excluded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PvtCandidateOutput {
    pub solution_kind: PvtEpochSolutionKind,
    pub ecef: Ecef,
    pub lla_rad: [f64; 3],
    pub receiver_clock_bias_s: f64,
    pub velocity: Option<PvtVelocityOutput>,
    pub quality: PvtQualityOutput,
    pub satellites: Vec<PvtSatelliteOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PvtCandidateStatusOutput {
    pub solution_kind: PvtEpochSolutionKind,
    pub disposition: PvtCandidateDisposition,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PvtEpochOutput {
    pub epoch_index: usize,
    pub solution_kind: PvtEpochSolutionKind,
    pub time: PvtTimeOutput,
    pub ecef: Ecef,
    pub lla_rad: [f64; 3],
    pub velocity: Option<PvtVelocityOutput>,
    pub quality: PvtQualityOutput,
    pub satellites: Vec<PvtSatelliteOutput>,
    pub candidates: Vec<PvtCandidateOutput>,
    pub candidate_statuses: Vec<PvtCandidateStatusOutput>,
}

impl ContinuousPvtEpochResult {
    pub fn to_output(&self) -> PvtEpochOutput {
        let selection = self.selected_solution();
        let receiver_clock_bias_s = selection.clock_bias_s.unwrap_or(0.0);
        let receiver_clock_drift_sps = selection
            .velocity
            .as_ref()
            .map(|velocity| velocity.clock_drift_sps);
        PvtEpochOutput {
            epoch_index: self.epoch_index,
            solution_kind: selection.kind,
            time: self.time_output(receiver_clock_bias_s, receiver_clock_drift_sps),
            ecef: selection.ecef.unwrap_or([0.0; 3]),
            lla_rad: selection.lla.unwrap_or([0.0; 3]),
            velocity: selection.velocity,
            quality: selection.quality,
            satellites: selection.satellites,
            candidates: self.candidate_outputs(),
            candidate_statuses: self.candidate_status_outputs(selection.kind),
        }
    }

    fn candidate_outputs(&self) -> Vec<PvtCandidateOutput> {
        let mut candidates = Vec::with_capacity(3);
        candidates.push(PvtCandidateOutput {
            solution_kind: PvtEpochSolutionKind::Raw,
            ecef: self.raw_solution.ecef,
            lla_rad: self.raw_solution.lla,
            receiver_clock_bias_s: self.raw_solution.clock_bias_s,
            velocity: self
                .raw_velocity_solution
                .as_ref()
                .map(PvtVelocityOutput::from_velocity),
            quality: quality_from_position(&self.raw_solution, None),
            satellites: satellites_from_position(&self.raw_solution),
        });
        if let Some(smoothed) = self.smoothed_candidate_solution.as_ref() {
            candidates.push(PvtCandidateOutput {
                solution_kind: PvtEpochSolutionKind::Smoothed,
                ecef: smoothed.ecef,
                lla_rad: smoothed.lla,
                receiver_clock_bias_s: smoothed.clock_bias_s,
                velocity: self
                    .smoothed_velocity_solution
                    .as_ref()
                    .map(PvtVelocityOutput::from_velocity),
                quality: quality_from_position(smoothed, None),
                satellites: satellites_from_position(smoothed),
            });
        }
        if let Some(kalman) = self.kalman_solution.as_ref() {
            let position = self
                .smoothed_solution
                .as_ref()
                .filter(|solution| solution.converged)
                .unwrap_or(&self.raw_solution);
            candidates.push(PvtCandidateOutput {
                solution_kind: PvtEpochSolutionKind::Kalman,
                ecef: kalman.ecef,
                lla_rad: kalman.lla,
                receiver_clock_bias_s: kalman.clock_bias_s,
                velocity: Some(PvtVelocityOutput::from_kalman(kalman)),
                quality: quality_from_position_and_kalman(position, kalman),
                satellites: satellites_from_position(position),
            });
        }
        candidates
    }

    fn candidate_status_outputs(
        &self,
        selected_kind: PvtEpochSolutionKind,
    ) -> Vec<PvtCandidateStatusOutput> {
        [
            (PvtEpochSolutionKind::Raw, self.raw_candidate_disposition),
            (
                PvtEpochSolutionKind::Smoothed,
                self.smoothed_candidate_disposition,
            ),
            (
                PvtEpochSolutionKind::Kalman,
                self.kalman_candidate_disposition,
            ),
        ]
        .into_iter()
        .map(|(solution_kind, disposition)| PvtCandidateStatusOutput {
            solution_kind,
            disposition: if solution_kind == selected_kind {
                PvtCandidateDisposition::Selected
            } else {
                disposition
            },
        })
        .collect()
    }

    fn selected_solution(&self) -> SelectedEpochSolution {
        if let Some(kalman) = self
            .kalman_solution
            .as_ref()
            .filter(|solution| solution.converged)
        {
            let position = self
                .smoothed_solution
                .as_ref()
                .filter(|solution| solution.converged)
                .unwrap_or(&self.raw_solution);
            return SelectedEpochSolution {
                kind: PvtEpochSolutionKind::Kalman,
                ecef: Some(kalman.ecef),
                lla: Some(kalman.lla),
                clock_bias_s: Some(kalman.clock_bias_s),
                velocity: Some(PvtVelocityOutput::from_kalman(kalman)),
                quality: quality_from_position_and_kalman(position, kalman),
                satellites: satellites_from_position(position),
            };
        }
        if let Some(smoothed) = self
            .smoothed_solution
            .as_ref()
            .filter(|solution| solution.converged)
        {
            return SelectedEpochSolution {
                kind: PvtEpochSolutionKind::Smoothed,
                ecef: Some(smoothed.ecef),
                lla: Some(smoothed.lla),
                clock_bias_s: Some(smoothed.clock_bias_s),
                velocity: self
                    .smoothed_velocity_solution
                    .as_ref()
                    .map(PvtVelocityOutput::from_velocity),
                quality: quality_from_position(smoothed, None),
                satellites: satellites_from_position(smoothed),
            };
        }
        if self.raw_solution.converged {
            return SelectedEpochSolution {
                kind: PvtEpochSolutionKind::Raw,
                ecef: Some(self.raw_solution.ecef),
                lla: Some(self.raw_solution.lla),
                clock_bias_s: Some(self.raw_solution.clock_bias_s),
                velocity: self
                    .raw_velocity_solution
                    .as_ref()
                    .map(PvtVelocityOutput::from_velocity),
                quality: quality_from_position(&self.raw_solution, None),
                satellites: satellites_from_position(&self.raw_solution),
            };
        }
        SelectedEpochSolution {
            kind: PvtEpochSolutionKind::Unconverged,
            ecef: None,
            lla: None,
            clock_bias_s: None,
            velocity: None,
            quality: quality_from_position(&self.raw_solution, None),
            satellites: satellites_from_position(&self.raw_solution),
        }
    }

    fn time_output(
        &self,
        receiver_clock_bias_s: f64,
        receiver_clock_drift_sps: Option<f64>,
    ) -> PvtTimeOutput {
        let receiver_time_sow = mean_receive_time_sow(&self.raw_pseudoranges);
        let corrected_gps_time = self
            .gps_week
            .zip(receiver_time_sow)
            .and_then(|(week, sow)| normalize_gps_time(week, sow - receiver_clock_bias_s));
        let corrected_gps_time_sow = corrected_gps_time
            .map(|time| time.sow)
            .or_else(|| receiver_time_sow.map(|sow| wrap_gps_sow(sow - receiver_clock_bias_s)));
        let gps_week = corrected_gps_time.map(|time| time.week).or(self.gps_week);
        let (utc_unix_seconds, utc) = corrected_gps_time
            .and_then(|time| gps_time_to_utc(time.week, time.sow))
            .map(|(unix_seconds, utc)| (Some(unix_seconds), Some(utc)))
            .unwrap_or((None, None));
        let transmit_time_sow_min = self
            .raw_pseudoranges
            .iter()
            .map(|pr| pr.transmit_time_sow)
            .reduce(f64::min);
        let transmit_time_sow_max = self
            .raw_pseudoranges
            .iter()
            .map(|pr| pr.transmit_time_sow)
            .reduce(f64::max);
        PvtTimeOutput {
            gps_week,
            receiver_time_sow,
            corrected_gps_time_sow,
            utc_unix_seconds,
            utc,
            transmit_time_sow_min,
            transmit_time_sow_max,
            receiver_clock_bias_s,
            receiver_clock_drift_sps,
        }
    }
}

impl PvtEpochOutput {
    pub fn from_results(results: &[ContinuousPvtEpochResult]) -> Vec<Self> {
        results
            .iter()
            .map(ContinuousPvtEpochResult::to_output)
            .collect()
    }
}

impl PvtVelocityOutput {
    fn from_velocity(solution: &VelocitySolution) -> Self {
        let speed_mps = velocity_speed_mps(solution);
        let horizontal_speed_mps =
            (solution.enu_velocity_mps[0].powi(2) + solution.enu_velocity_mps[1].powi(2)).sqrt();
        let heading_rad = (horizontal_speed_mps > 1e-6)
            .then(|| solution.enu_velocity_mps[0].atan2(solution.enu_velocity_mps[1]));
        Self {
            ecef_velocity_mps: solution.ecef_velocity_mps,
            enu_velocity_mps: solution.enu_velocity_mps,
            speed_mps,
            horizontal_speed_mps,
            vertical_speed_mps: solution.enu_velocity_mps[2],
            heading_rad,
            clock_drift_mps: solution.clock_drift_mps,
            clock_drift_sps: solution.clock_drift_sps,
            residual_rms_mps: residual_rms(&solution.residuals_mps).unwrap_or(0.0),
            used_prns: solution.used_prns.clone(),
        }
    }

    fn from_kalman(solution: &KalmanSolution) -> Self {
        let speed_mps = norm3(solution.ecef_velocity_mps);
        let horizontal_speed_mps =
            (solution.enu_velocity_mps[0].powi(2) + solution.enu_velocity_mps[1].powi(2)).sqrt();
        let heading_rad = (horizontal_speed_mps > 1e-6)
            .then(|| solution.enu_velocity_mps[0].atan2(solution.enu_velocity_mps[1]));
        Self {
            ecef_velocity_mps: solution.ecef_velocity_mps,
            enu_velocity_mps: solution.enu_velocity_mps,
            speed_mps,
            horizontal_speed_mps,
            vertical_speed_mps: solution.enu_velocity_mps[2],
            heading_rad,
            clock_drift_mps: solution.clock_drift_sps * C_LIGHT,
            clock_drift_sps: solution.clock_drift_sps,
            residual_rms_mps: 0.0,
            used_prns: Vec::new(),
        }
    }
}

struct SelectedEpochSolution {
    kind: PvtEpochSolutionKind,
    ecef: Option<Ecef>,
    lla: Option<[f64; 3]>,
    clock_bias_s: Option<f64>,
    velocity: Option<PvtVelocityOutput>,
    quality: PvtQualityOutput,
    satellites: Vec<PvtSatelliteOutput>,
}

fn satellites_from_position(position: &PvtSolution) -> Vec<PvtSatelliteOutput> {
    position
        .satellite_diagnostics
        .iter()
        .map(|diagnostic| PvtSatelliteOutput {
            prn: diagnostic.prn,
            azimuth_rad: diagnostic.azimuth_rad,
            elevation_rad: diagnostic.elevation_rad,
            residual_m: diagnostic.residual_m,
            standardized_residual: diagnostic.standardized_residual,
            pseudorange_sigma_m: diagnostic.pseudorange_sigma_m,
            used_in_solution: true,
            raim_excluded: false,
        })
        .collect()
}

fn quality_from_position(
    position: &PvtSolution,
    velocity_sigma_mps: Option<f64>,
) -> PvtQualityOutput {
    PvtQualityOutput {
        converged: position.converged,
        n_sats: position.n_sats,
        used_prns: position.used_prns.clone(),
        excluded_prns: position.excluded_prns.clone(),
        residual_rms_m: position.residual_rms_m,
        standardized_residual_rms: position.standardized_residual_rms,
        max_standardized_residual: position.max_standardized_residual,
        epoch_quality_scale: position.epoch_quality_scale,
        hdop: position.hdop,
        vdop: position.vdop,
        pdop: position.pdop,
        gdop: position.gdop,
        horizontal_sigma_m: position.horizontal_sigma_m,
        vertical_sigma_m: position.vertical_sigma_m,
        position_sigma_m: position.position_sigma_m,
        velocity_sigma_mps,
    }
}

fn quality_from_position_and_kalman(
    position: &PvtSolution,
    kalman: &KalmanSolution,
) -> PvtQualityOutput {
    let mut quality = quality_from_position(position, Some(kalman.velocity_std_mps));
    quality.position_sigma_m = kalman.position_std_m;
    quality.velocity_sigma_mps = Some(kalman.velocity_std_mps);
    quality
}

#[derive(Debug, Clone)]
pub struct ContinuousPvtRunner {
    config: ContinuousPvtConfig,
    hatch: Option<HatchSmoother>,
    kalman: Option<PositionVelocityKalman>,
    last_accepted_position: Option<AcceptedPosition>,
}

#[derive(Debug, Clone, Copy)]
struct AcceptedPosition {
    ecef: Ecef,
    receive_time_sow: f64,
}

impl ContinuousPvtRunner {
    pub fn new(config: ContinuousPvtConfig) -> Self {
        let hatch = config.hatch.clone().map(HatchSmoother::new);
        let kalman = config.kalman.clone().map(PositionVelocityKalman::new);
        Self {
            config,
            hatch,
            kalman,
            last_accepted_position: None,
        }
    }

    pub fn solve_epochs(
        &mut self,
        epochs: &[PseudorangeEpoch],
        ephemerides: &EphemerisMap,
        iono: &IonoParams,
        rx_lat_rad: f64,
        rx_lon_rad: f64,
        rx_alt_m: f64,
    ) -> Vec<ContinuousPvtEpochResult> {
        let wls = self.config.wls.clone();
        let apriori_ecef = has_apriori(rx_lat_rad, rx_lon_rad, rx_alt_m)
            .then(|| lla_to_ecef([rx_lat_rad, rx_lon_rad, rx_alt_m]))
            .or_else(|| self.last_accepted_position.map(|position| position.ecef))
            .or_else(|| self.config.initial_position_lla_rad.map(lla_to_ecef));
        let solver_apriori = apriori_ecef
            .map(ecef_to_lla)
            .unwrap_or([rx_lat_rad, rx_lon_rad, rx_alt_m]);
        let ambiguity = self.config.integer_ms_ambiguity.clone();
        self.solve_epochs_with_preprocess(
            epochs,
            consistent_gps_week(ephemerides, self.config.gps_week_rollover_reference),
            |pseudoranges| {
                solve_wls_with_config(
                    pseudoranges,
                    ephemerides,
                    iono,
                    solver_apriori[0],
                    solver_apriori[1],
                    solver_apriori[2],
                    &wls,
                )
            },
            |pseudoranges| match (ambiguity.as_ref(), apriori_ecef) {
                (Some(config), Some(ecef)) => {
                    let predicted =
                        predicted_pseudoranges_for_ambiguity(pseudoranges, ephemerides, ecef);
                    resolve_integer_ms_ambiguities(pseudoranges, &predicted, config.max_shift_ms)
                }
                _ => pseudoranges.to_vec(),
            },
            |pseudoranges, solution| {
                Some(solve_doppler_velocity(
                    pseudoranges,
                    ephemerides,
                    solution.ecef,
                ))
            },
        )
    }

    pub fn solve_epochs_with_catalog(
        &mut self,
        epochs: &[PseudorangeEpoch],
        ephemeris_catalog: &EphemerisCatalog,
        iono: &IonoParams,
        rx_lat_rad: f64,
        rx_lon_rad: f64,
        rx_alt_m: f64,
    ) -> Vec<ContinuousPvtEpochResult> {
        let wls = self.config.wls.clone();
        let selection_config = self.config.ephemeris_selection.clone();
        let apriori_ecef = has_apriori(rx_lat_rad, rx_lon_rad, rx_alt_m)
            .then(|| lla_to_ecef([rx_lat_rad, rx_lon_rad, rx_alt_m]))
            .or_else(|| self.last_accepted_position.map(|position| position.ecef))
            .or_else(|| self.config.initial_position_lla_rad.map(lla_to_ecef));
        let solver_apriori = apriori_ecef
            .map(ecef_to_lla)
            .unwrap_or([rx_lat_rad, rx_lon_rad, rx_alt_m]);
        let ambiguity = self.config.integer_ms_ambiguity.clone();
        self.solve_epochs_with_preprocess_and_solve(
            epochs,
            None,
            |pseudoranges| {
                ephemeris_map_for_pseudoranges(pseudoranges, ephemeris_catalog, &selection_config)
            },
            |pseudoranges, ephemerides| {
                solve_wls_with_config(
                    pseudoranges,
                    ephemerides,
                    iono,
                    solver_apriori[0],
                    solver_apriori[1],
                    solver_apriori[2],
                    &wls,
                )
            },
            |pseudoranges, ephemerides| match (ambiguity.as_ref(), apriori_ecef) {
                (Some(config), Some(ecef)) => {
                    let predicted =
                        predicted_pseudoranges_for_ambiguity(pseudoranges, ephemerides, ecef);
                    resolve_integer_ms_ambiguities(pseudoranges, &predicted, config.max_shift_ms)
                }
                _ => pseudoranges.to_vec(),
            },
            |pseudoranges, solution, ephemerides| {
                Some(solve_doppler_velocity(
                    pseudoranges,
                    ephemerides,
                    solution.ecef,
                ))
            },
        )
    }

    pub fn solve_epochs_with<F>(
        &mut self,
        epochs: &[PseudorangeEpoch],
        solve: F,
    ) -> Vec<ContinuousPvtEpochResult>
    where
        F: FnMut(&[Pseudorange]) -> PvtSolution,
    {
        self.solve_epochs_with_preprocess(
            epochs,
            None,
            solve,
            |pseudoranges| pseudoranges.to_vec(),
            |_, _| None,
        )
    }

    pub fn solve_epochs_with_velocity<F, V>(
        &mut self,
        epochs: &[PseudorangeEpoch],
        solve: F,
        velocity: V,
    ) -> Vec<ContinuousPvtEpochResult>
    where
        F: FnMut(&[Pseudorange]) -> PvtSolution,
        V: FnMut(&[Pseudorange], &PvtSolution) -> Option<VelocitySolution>,
    {
        self.solve_epochs_with_preprocess(
            epochs,
            None,
            solve,
            |pseudoranges| pseudoranges.to_vec(),
            velocity,
        )
    }

    fn solve_epochs_with_preprocess<F, P, V>(
        &mut self,
        epochs: &[PseudorangeEpoch],
        fixed_gps_week: Option<u16>,
        mut solve: F,
        mut preprocess: P,
        mut velocity: V,
    ) -> Vec<ContinuousPvtEpochResult>
    where
        F: FnMut(&[Pseudorange]) -> PvtSolution,
        P: FnMut(&[Pseudorange]) -> Vec<Pseudorange>,
        V: FnMut(&[Pseudorange], &PvtSolution) -> Option<VelocitySolution>,
    {
        self.solve_epochs_with_preprocess_and_solve(
            epochs,
            fixed_gps_week,
            |_| EphemerisMap::new(),
            |pseudoranges, _| solve(pseudoranges),
            |pseudoranges, _| preprocess(pseudoranges),
            |pseudoranges, solution, _| velocity(pseudoranges, solution),
        )
    }

    fn solve_epochs_with_preprocess_and_solve<S, F, P, V>(
        &mut self,
        epochs: &[PseudorangeEpoch],
        fixed_gps_week: Option<u16>,
        mut select_ephemerides: S,
        mut solve: F,
        mut preprocess: P,
        mut velocity: V,
    ) -> Vec<ContinuousPvtEpochResult>
    where
        S: FnMut(&[Pseudorange]) -> EphemerisMap,
        F: FnMut(&[Pseudorange], &EphemerisMap) -> PvtSolution,
        P: FnMut(&[Pseudorange], &EphemerisMap) -> Vec<Pseudorange>,
        V: FnMut(&[Pseudorange], &PvtSolution, &EphemerisMap) -> Option<VelocitySolution>,
    {
        let position_quality = self.config.position_quality.clone();
        let mut last_accepted_position = self.last_accepted_position;
        let results = epochs
            .iter()
            .enumerate()
            .map(|(epoch_index, epoch)| {
                let ephemerides = select_ephemerides(&epoch.pseudoranges);
                let gps_week =
                    consistent_gps_week(&ephemerides, self.config.gps_week_rollover_reference)
                        .or(fixed_gps_week);
                let raw_pseudoranges = preprocess(&epoch.pseudoranges, &ephemerides);
                let mut solve_raw =
                    |pseudoranges: &[Pseudorange]| solve(pseudoranges, &ephemerides);
                let mut raw_solution =
                    solve_if_enough_satellites(&raw_pseudoranges, &mut solve_raw);
                let receive_time_sow = mean_receive_time_sow(&raw_pseudoranges);
                let raw_candidate_disposition = if !raw_solution.converged {
                    PvtCandidateDisposition::NonConverged
                } else if !position_solution_is_acceptable(
                    &raw_solution,
                    receive_time_sow,
                    last_accepted_position,
                    &position_quality,
                ) {
                    raw_solution.converged = false;
                    PvtCandidateDisposition::QualityRejected
                } else if let Some(receive_time_sow) = receive_time_sow {
                    last_accepted_position = Some(AcceptedPosition {
                        ecef: raw_solution.ecef,
                        receive_time_sow,
                    });
                    PvtCandidateDisposition::Available
                } else {
                    PvtCandidateDisposition::Available
                };
                let raw_velocity = velocity_if_position_converged(
                    &raw_pseudoranges,
                    &raw_solution,
                    &mut |pseudoranges, solution| velocity(pseudoranges, solution, &ephemerides),
                    &self.config.velocity_quality,
                );
                let raw_velocity_solution = raw_velocity.accepted;
                let raw_velocity_for_kalman = raw_velocity.kalman;

                let (
                    smoothed_pseudoranges,
                    smoothed_solution,
                    smoothed_candidate_solution,
                    smoothed_candidate_disposition,
                    smoothed_velocity_solution,
                    smoothed_velocity_for_kalman,
                ) = match self.hatch.as_mut() {
                    Some(hatch) if raw_solution.converged => {
                        let smoothed = hatch.smooth_epoch(&raw_pseudoranges);
                        let smoothed_ephemerides = select_ephemerides(&smoothed);
                        let mut solve_smoothed = |pseudoranges: &[Pseudorange]| {
                            solve(pseudoranges, &smoothed_ephemerides)
                        };
                        let solution = solve_if_enough_satellites(&smoothed, &mut solve_smoothed);
                        if smoothed_solution_is_acceptable(&raw_solution, &solution, hatch.config())
                        {
                            let velocity_result = velocity_if_position_converged(
                                &smoothed,
                                &solution,
                                &mut |pseudoranges, solution| {
                                    velocity(pseudoranges, solution, &smoothed_ephemerides)
                                },
                                &self.config.velocity_quality,
                            );
                            (
                                Some(smoothed),
                                Some(solution.clone()),
                                Some(solution),
                                PvtCandidateDisposition::Available,
                                velocity_result.accepted,
                                velocity_result.kalman,
                            )
                        } else {
                            let disposition = if solution.converged {
                                PvtCandidateDisposition::QualityRejected
                            } else {
                                PvtCandidateDisposition::NonConverged
                            };
                            if hatch.config().reset_on_rejected_solution {
                                hatch.reset();
                            }
                            (
                                Some(smoothed),
                                None,
                                Some(solution),
                                disposition,
                                None,
                                None,
                            )
                        }
                    }
                    Some(hatch) => {
                        hatch.reset();
                        (
                            None,
                            None,
                            None,
                            PvtCandidateDisposition::Unavailable,
                            None,
                            None,
                        )
                    }
                    None => (
                        None,
                        None,
                        None,
                        PvtCandidateDisposition::Unavailable,
                        None,
                        None,
                    ),
                };
                let kalman_solution = update_kalman(
                    self.kalman.as_mut(),
                    &raw_pseudoranges,
                    &raw_solution,
                    raw_velocity_for_kalman.as_ref(),
                    smoothed_solution.as_ref(),
                    smoothed_velocity_for_kalman.as_ref(),
                );
                let kalman_candidate_disposition = match kalman_solution.as_ref() {
                    Some(solution) if solution.converged => PvtCandidateDisposition::Available,
                    _ if self.kalman.is_some() => PvtCandidateDisposition::NonConverged,
                    _ => PvtCandidateDisposition::Unavailable,
                };

                ContinuousPvtEpochResult {
                    epoch_index,
                    gps_week,
                    raw_pseudoranges,
                    smoothed_pseudoranges,
                    raw_solution,
                    raw_candidate_disposition,
                    raw_velocity_solution,
                    smoothed_solution,
                    smoothed_candidate_solution,
                    smoothed_candidate_disposition,
                    smoothed_velocity_solution,
                    kalman_solution,
                    kalman_candidate_disposition,
                }
            })
            .collect();
        self.last_accepted_position = last_accepted_position;
        results
    }
}

fn consistent_gps_week(ephemerides: &EphemerisMap, rollover_reference: Option<u16>) -> Option<u16> {
    let mut counts = BTreeMap::<u16, usize>::new();
    for week in ephemerides.values().filter_map(|ephemeris| {
        if ephemeris.week_number_is_truncated {
            rollover_reference
                .and_then(|reference| expand_truncated_gps_week(ephemeris.week_number, reference))
        } else {
            Some(ephemeris.week_number)
        }
    }) {
        *counts.entry(week).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(week, count)| (*count, *week))
        .map(|(week, _)| week)
}

fn expand_truncated_gps_week(truncated_week: u16, reference_week: u16) -> Option<u16> {
    if truncated_week >= 1024 {
        return None;
    }
    let reference = i64::from(reference_week);
    let truncated = i64::from(truncated_week);
    let rollover = ((reference - truncated) as f64 / 1024.0).round() as i64;
    let expanded = truncated + rollover * 1024;
    u16::try_from(expanded).ok()
}

fn ephemeris_map_for_pseudoranges(
    pseudoranges: &[Pseudorange],
    catalog: &EphemerisCatalog,
    config: &EphemerisSelectionConfig,
) -> EphemerisMap {
    let transmit_times: Vec<(u8, f64)> = pseudoranges
        .iter()
        .map(|pr| (pr.prn, pr.transmit_time_sow))
        .collect();
    ephemeris_map_from_catalog_at_time(catalog, &transmit_times, config)
}

fn update_kalman(
    kalman: Option<&mut PositionVelocityKalman>,
    raw_pseudoranges: &[Pseudorange],
    raw_solution: &PvtSolution,
    raw_velocity: Option<&VelocitySolution>,
    smoothed_solution: Option<&PvtSolution>,
    smoothed_velocity: Option<&VelocitySolution>,
) -> Option<KalmanSolution> {
    let kalman = kalman?;
    let epoch_time_sow = raw_pseudoranges.first()?.receive_time_sow;
    let (position, velocity) = match smoothed_solution {
        Some(solution) if solution.converged => (solution, smoothed_velocity.or(raw_velocity)),
        _ => (raw_solution, raw_velocity),
    };
    kalman.update(epoch_time_sow, position, velocity)
}

fn has_apriori(rx_lat_rad: f64, rx_lon_rad: f64, rx_alt_m: f64) -> bool {
    rx_lat_rad.abs() > 1e-12 || rx_lon_rad.abs() > 1e-12 || rx_alt_m.abs() > 1e-12
}

fn predicted_pseudoranges_for_ambiguity(
    pseudoranges: &[Pseudorange],
    ephemerides: &EphemerisMap,
    apriori_ecef: Ecef,
) -> Vec<(u8, f64)> {
    pseudoranges
        .iter()
        .filter_map(|pr| {
            let eph = ephemerides.get(&pr.prn)?;
            let sat = satellite_position(eph, pr.transmit_time_sow);
            let tau = pr.pseudorange_m / C_LIGHT;
            let theta = OMEGA_E * tau;
            let xs_r = theta.cos() * sat.ecef[0] + theta.sin() * sat.ecef[1];
            let ys_r = -theta.sin() * sat.ecef[0] + theta.cos() * sat.ecef[1];
            let zs_r = sat.ecef[2];
            let dx = xs_r - apriori_ecef[0];
            let dy = ys_r - apriori_ecef[1];
            let dz = zs_r - apriori_ecef[2];
            let range = (dx * dx + dy * dy + dz * dz).sqrt();
            Some((pr.prn, range - C_LIGHT * sat.clock_correction_s))
        })
        .collect()
}

fn solve_if_enough_satellites<F>(pseudoranges: &[Pseudorange], solve: &mut F) -> PvtSolution
where
    F: FnMut(&[Pseudorange]) -> PvtSolution,
{
    if pseudoranges.len() < 4 {
        return PvtSolution {
            n_sats: pseudoranges.len(),
            ..Default::default()
        };
    }
    solve(pseudoranges)
}

fn velocity_if_position_converged<V>(
    pseudoranges: &[Pseudorange],
    solution: &PvtSolution,
    velocity: &mut V,
    config: &VelocityQualityConfig,
) -> VelocityQualityResult
where
    V: FnMut(&[Pseudorange], &PvtSolution) -> Option<VelocitySolution>,
{
    if !solution.converged {
        return VelocityQualityResult::default();
    }
    let Some(velocity_solution) =
        velocity(pseudoranges, solution).filter(|velocity_solution| velocity_solution.converged)
    else {
        return VelocityQualityResult::default();
    };
    if velocity_solution_is_acceptable(&velocity_solution, config) {
        return VelocityQualityResult {
            accepted: Some(velocity_solution.clone()),
            kalman: Some(velocity_solution),
        };
    }
    VelocityQualityResult {
        accepted: None,
        kalman: config
            .rejected_velocity_fallback_sigma_mps
            .map(zero_velocity_fallback),
    }
}

fn position_solution_is_acceptable(
    solution: &PvtSolution,
    receive_time_sow: Option<f64>,
    previous: Option<AcceptedPosition>,
    config: &PositionQualityConfig,
) -> bool {
    if !solution.converged
        || solution.ecef.iter().any(|value| !value.is_finite())
        || solution.lla.iter().any(|value| !value.is_finite())
        || !solution.residual_rms_m.is_finite()
        || solution.residual_rms_m > config.max_residual_rms_m.max(0.0)
        || !(config.min_altitude_m..=config.max_altitude_m).contains(&solution.lla[2])
    {
        return false;
    }
    let Some((receive_time_sow, previous)) = receive_time_sow.zip(previous) else {
        return true;
    };
    let delta_time_s = wrapped_gps_time_delta_s(receive_time_sow, previous.receive_time_sow);
    if delta_time_s <= 0.0 {
        return true;
    }
    let delta_position_m = norm3([
        solution.ecef[0] - previous.ecef[0],
        solution.ecef[1] - previous.ecef[1],
        solution.ecef[2] - previous.ecef[2],
    ]);
    let maximum_jump_m =
        config.jump_margin_m.max(0.0) + config.max_implied_speed_mps.max(0.0) * delta_time_s;
    delta_position_m <= maximum_jump_m
}

fn wrapped_gps_time_delta_s(current_sow: f64, previous_sow: f64) -> f64 {
    let delta = (current_sow - previous_sow).rem_euclid(604_800.0);
    if delta > 302_400.0 {
        delta - 604_800.0
    } else {
        delta
    }
}

fn smoothed_solution_is_acceptable(
    raw_solution: &PvtSolution,
    smoothed_solution: &PvtSolution,
    config: &HatchConfig,
) -> bool {
    if !smoothed_solution.converged {
        return false;
    }
    let Some(raw_rms_m) = residual_rms(&raw_solution.residuals_m) else {
        return true;
    };
    let Some(smoothed_rms_m) = residual_rms(&smoothed_solution.residuals_m) else {
        return true;
    };
    smoothed_rms_m
        <= raw_rms_m * config.max_smoothed_residual_rms_ratio
            + config.max_smoothed_residual_rms_margin_m
}

fn residual_rms(residuals_m: &[f64]) -> Option<f64> {
    if residuals_m.is_empty() {
        return None;
    }
    Some(
        (residuals_m
            .iter()
            .map(|residual| residual * residual)
            .sum::<f64>()
            / residuals_m.len() as f64)
            .sqrt(),
    )
}

fn velocity_solution_is_acceptable(
    solution: &VelocitySolution,
    config: &VelocityQualityConfig,
) -> bool {
    if solution
        .residuals_mps
        .iter()
        .any(|residual| !residual.is_finite())
    {
        return false;
    }
    if let Some(rms_mps) = residual_rms(&solution.residuals_mps) {
        if rms_mps > config.max_residual_rms_mps {
            return false;
        }
    }
    if solution.clock_drift_mps.abs() > config.max_clock_drift_mps {
        return false;
    }
    velocity_speed_mps(solution) <= config.max_speed_mps
}

fn velocity_speed_mps(solution: &VelocitySolution) -> f64 {
    norm3(solution.ecef_velocity_mps)
}

fn norm3(vector: Ecef) -> f64 {
    (vector[0].powi(2) + vector[1].powi(2) + vector[2].powi(2)).sqrt()
}

fn mean_receive_time_sow(pseudoranges: &[Pseudorange]) -> Option<f64> {
    if pseudoranges.is_empty() {
        return None;
    }
    Some(
        pseudoranges
            .iter()
            .map(|pr| pr.receive_time_sow)
            .sum::<f64>()
            / pseudoranges.len() as f64,
    )
}

fn wrap_gps_sow(sow: f64) -> f64 {
    sow.rem_euclid(604_800.0)
}

#[derive(Debug, Clone, Default)]
struct VelocityQualityResult {
    accepted: Option<VelocitySolution>,
    kalman: Option<VelocitySolution>,
}

fn zero_velocity_fallback(sigma_mps: f64) -> VelocitySolution {
    let sigma_mps = sigma_mps.max(0.0);
    VelocitySolution {
        ecef_velocity_mps: [0.0; 3],
        enu_velocity_mps: [0.0; 3],
        residuals_mps: vec![sigma_mps],
        converged: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pvt::PvtSatelliteDiagnostic;

    #[test]
    fn test_continuous_runner_solves_raw_and_smoothed_epochs() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            hatch: Some(HatchConfig {
                window_epochs: 10,
                max_gap_s: 2.0,
                divergence_reset_threshold_m: 1_000.0,
                ..Default::default()
            }),
            ..Default::default()
        });
        let epochs = vec![
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 100.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_100.0, 101.0),
            },
        ];

        let mut calls = 0usize;
        let results = runner.solve_epochs_with(&epochs, |pseudoranges| {
            calls += 1;
            solved(pseudoranges.len())
        });

        assert_eq!(results.len(), 2);
        assert_eq!(calls, 4);
        assert!(results.iter().all(|result| result.raw_solution.converged));
        assert!(results.iter().all(|result| result
            .smoothed_solution
            .as_ref()
            .is_some_and(|solution| solution.converged)));
        let smoothed_second = results[1].smoothed_pseudoranges.as_ref().unwrap();
        assert_eq!(smoothed_second.len(), 4);
        assert!(smoothed_second[0].pseudorange_m < epochs[1].pseudoranges[0].pseudorange_m);
    }

    #[test]
    fn test_continuous_runner_retains_unsolved_short_epochs() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig::default());
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: vec![
                pseudorange(1, 20_000_000.0, 100.0),
                pseudorange(2, 20_100_000.0, 100.0),
                pseudorange(3, 20_200_000.0, 100.0),
            ],
        }];

        let mut calls = 0usize;
        let results = runner.solve_epochs_with(&epochs, |pseudoranges| {
            calls += 1;
            solved(pseudoranges.len())
        });

        assert_eq!(results.len(), 1);
        assert_eq!(calls, 0);
        assert_eq!(results[0].epoch_index, 0);
        assert_eq!(results[0].raw_solution.n_sats, 3);
        assert!(!results[0].raw_solution.converged);
        assert!(results[0].raw_velocity_solution.is_none());
        assert!(results[0].smoothed_solution.is_none());
        assert!(results[0].smoothed_velocity_solution.is_none());
        let statuses = results[0].to_output().candidate_statuses;
        assert_eq!(
            statuses,
            vec![
                PvtCandidateStatusOutput {
                    solution_kind: PvtEpochSolutionKind::Raw,
                    disposition: PvtCandidateDisposition::NonConverged,
                },
                PvtCandidateStatusOutput {
                    solution_kind: PvtEpochSolutionKind::Smoothed,
                    disposition: PvtCandidateDisposition::Unavailable,
                },
                PvtCandidateStatusOutput {
                    solution_kind: PvtEpochSolutionKind::Kalman,
                    disposition: PvtCandidateDisposition::Unavailable,
                },
            ]
        );
    }

    #[test]
    fn test_continuous_runner_outputs_ephemeris_week_and_cttc_utc_time() {
        let mut ephemerides = EphemerisMap::new();
        for prn in 1..=3 {
            ephemerides.insert(
                prn,
                crate::nav::Ephemeris {
                    prn,
                    week_number: 1734,
                    ..Default::default()
                },
            );
        }
        let epoch = PseudorangeEpoch {
            pseudoranges: (1..=3)
                .map(|prn| pseudorange(prn, 20_000_000.0, 368_663.0))
                .collect(),
        };
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig::default());

        let result = runner
            .solve_epochs(
                &[epoch],
                &ephemerides,
                &IonoParams::default(),
                0.0,
                0.0,
                0.0,
            )
            .pop()
            .expect("retained short epoch");
        let output = result.to_output();

        assert_eq!(result.gps_week, Some(1734));
        assert_eq!(output.time.gps_week, Some(1734));
        assert_eq!(output.time.utc_unix_seconds, Some(1_365_056_647.0));
        assert_eq!(
            output.time.utc,
            Some(UtcDateTime {
                year: 2013,
                month: 4,
                day: 4,
                hour: 6,
                minute: 24,
                second: 7.0,
            })
        );
    }

    #[test]
    fn test_truncated_gps_week_requires_and_uses_rollover_reference() {
        assert_eq!(expand_truncated_gps_week(710, 1734), Some(1734));
        assert_eq!(expand_truncated_gps_week(710, 710), Some(710));

        let ephemerides = EphemerisMap::from([(
            1,
            crate::nav::Ephemeris {
                prn: 1,
                week_number: 710,
                week_number_is_truncated: true,
                ..Default::default()
            },
        )]);
        assert_eq!(consistent_gps_week(&ephemerides, None), None);
        assert_eq!(consistent_gps_week(&ephemerides, Some(1734)), Some(1734));
    }

    #[test]
    fn test_continuous_runner_attaches_raw_and_smoothed_velocity() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            hatch: Some(HatchConfig {
                window_epochs: 10,
                max_gap_s: 2.0,
                divergence_reset_threshold_m: 1_000.0,
                ..Default::default()
            }),
            ..Default::default()
        });
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: four_prns(20_000_000.0, 100.0),
        }];

        let mut velocity_calls = 0usize;
        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |pseudoranges| solved(pseudoranges.len()),
            |pseudoranges, solution| {
                velocity_calls += 1;
                Some(velocity_solution(
                    pseudoranges.len(),
                    solution.ecef[0] + velocity_calls as f64,
                ))
            },
        );

        assert_eq!(velocity_calls, 2);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0]
                .raw_velocity_solution
                .as_ref()
                .unwrap()
                .used_prns
                .len(),
            4
        );
        assert_eq!(
            results[0]
                .smoothed_velocity_solution
                .as_ref()
                .unwrap()
                .used_prns
                .len(),
            4
        );
        assert_eq!(
            results[0]
                .raw_velocity_solution
                .as_ref()
                .unwrap()
                .ecef_velocity_mps[0],
            1.0
        );
        assert_eq!(
            results[0]
                .smoothed_velocity_solution
                .as_ref()
                .unwrap()
                .ecef_velocity_mps[0],
            2.0
        );
    }

    #[test]
    fn test_ephemeris_map_for_pseudoranges_selects_catalog_records_by_transmit_time() {
        let mut catalog = EphemerisCatalog::new();
        catalog.insert(
            1,
            vec![
                ephemeris_for_catalog_test(1, 1_000.0, 1),
                ephemeris_for_catalog_test(1, 2_000.0, 2),
            ],
        );
        catalog.insert(2, vec![ephemeris_for_catalog_test(2, 5_000.0, 7)]);
        let pseudoranges = vec![
            pseudorange(1, 20_000_000.0, 2_100.075),
            pseudorange(2, 20_000_000.0, 5_100.075),
        ];

        let selected = ephemeris_map_for_pseudoranges(&pseudoranges, &catalog, &Default::default());

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[&1].iode, 2);
        assert_eq!(selected[&2].iode, 7);
    }

    #[test]
    fn test_continuous_runner_rejects_velocity_above_configured_speed_cap() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            velocity_quality: VelocityQualityConfig {
                max_speed_mps: 2.0,
                ..Default::default()
            },
            ..Default::default()
        });
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: four_prns(20_000_000.0, 100.0),
        }];

        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |pseudoranges| solved(pseudoranges.len()),
            |_, _| Some(velocity_solution(4, 5.0)),
        );

        assert!(results[0].raw_solution.converged);
        assert!(results[0].raw_velocity_solution.is_none());
    }

    #[test]
    fn test_continuous_runner_can_feed_zero_velocity_fallback_to_kalman() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            kalman: Some(KalmanConfig::default()),
            velocity_quality: VelocityQualityConfig {
                max_speed_mps: 2.0,
                rejected_velocity_fallback_sigma_mps: Some(0.25),
                ..Default::default()
            },
            ..Default::default()
        });
        let epochs = vec![
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 100.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 101.0),
            },
        ];

        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |pseudoranges| solved(pseudoranges.len()),
            |_, _| Some(velocity_solution(4, 5.0)),
        );

        assert!(results
            .iter()
            .all(|result| result.raw_velocity_solution.is_none()));
        let kalman = results[1].kalman_solution.as_ref().unwrap();
        assert!(kalman.converged);
        assert!(kalman_speed_mps(kalman) < 0.5);
        assert!((kalman.velocity_measurement_sigma_mps - 0.25).abs() < 1e-12);
    }

    #[test]
    fn test_continuous_runner_rejects_velocity_with_large_residual_rms() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            velocity_quality: VelocityQualityConfig {
                max_residual_rms_mps: 0.5,
                ..Default::default()
            },
            ..Default::default()
        });
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: four_prns(20_000_000.0, 100.0),
        }];

        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |pseudoranges| solved(pseudoranges.len()),
            |_, _| {
                let mut velocity = velocity_solution(4, 1.0);
                velocity.residuals_mps = vec![0.0, 0.0, 1.0, -1.0];
                Some(velocity)
            },
        );

        assert!(results[0].raw_solution.converged);
        assert!(results[0].raw_velocity_solution.is_none());
    }

    #[test]
    fn test_continuous_runner_rejects_physically_implausible_position_jump() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig::default());
        let epochs = vec![
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 100.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 100.1),
            },
        ];
        let valid_ecef = lla_to_ecef([41.2748_f64.to_radians(), 1.9876_f64.to_radians(), 70.0]);
        let bad_lla = [
            39.5597_f64.to_radians(),
            -0.2507_f64.to_radians(),
            1_123_486.0,
        ];
        let bad_ecef = lla_to_ecef(bad_lla);
        let mut solve_calls = 0;

        let results = runner.solve_epochs_with(&epochs, |_| {
            solve_calls += 1;
            if solve_calls == 1 {
                PvtSolution {
                    ecef: valid_ecef,
                    lla: [41.2748_f64.to_radians(), 1.9876_f64.to_radians(), 70.0],
                    n_sats: 4,
                    converged: true,
                    ..Default::default()
                }
            } else {
                PvtSolution {
                    ecef: bad_ecef,
                    lla: bad_lla,
                    n_sats: 4,
                    residual_rms_m: 1e-8,
                    converged: true,
                    ..Default::default()
                }
            }
        });

        assert!(results[0].raw_solution.converged);
        assert!(!results[1].raw_solution.converged);
        assert_eq!(
            results[1].to_output().solution_kind,
            PvtEpochSolutionKind::Unconverged
        );
    }

    #[test]
    fn test_default_position_quality_rejects_stratospheric_cold_start() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig::default());
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: four_prns(20_000_000.0, 100.0),
        }];
        let bad_lla = [38.1403_f64.to_radians(), 0.5290_f64.to_radians(), 90_225.0];

        let results = runner.solve_epochs_with(&epochs, |_| PvtSolution {
            ecef: lla_to_ecef(bad_lla),
            lla: bad_lla,
            n_sats: 5,
            residual_rms_m: 116_622.0,
            converged: true,
            ..Default::default()
        });

        assert!(!results[0].raw_solution.converged);
        assert_eq!(
            results[0].raw_candidate_disposition,
            PvtCandidateDisposition::QualityRejected
        );
    }

    #[test]
    fn test_default_position_quality_rejects_large_postfit_residual() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig::default());
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: four_prns(20_000_000.0, 100.0),
        }];
        let lla = [41.2747_f64.to_radians(), 1.9875_f64.to_radians(), 70.0];

        let results = runner.solve_epochs_with(&epochs, |_| PvtSolution {
            ecef: lla_to_ecef(lla),
            lla,
            n_sats: 5,
            residual_rms_m: 116_622.0,
            converged: true,
            ..Default::default()
        });

        assert!(!results[0].raw_solution.converged);
    }

    #[test]
    fn test_continuous_runner_resets_smoothing_after_failed_raw_solution() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            hatch: Some(HatchConfig {
                window_epochs: 10,
                max_gap_s: 2.0,
                divergence_reset_threshold_m: 10_000.0,
                ..Default::default()
            }),
            ..Default::default()
        });
        let epochs = vec![
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 100.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_100.0, 101.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_200.0, 102.0),
            },
        ];

        let mut calls = 0usize;
        let results = runner.solve_epochs_with(&epochs, |pseudoranges| {
            calls += 1;
            if calls == 3 {
                PvtSolution {
                    n_sats: pseudoranges.len(),
                    converged: false,
                    ..Default::default()
                }
            } else {
                solved(pseudoranges.len())
            }
        });

        assert_eq!(results.len(), 3);
        assert!(results[0].raw_solution.converged);
        assert!(results[0].smoothed_solution.is_some());
        assert!(!results[1].raw_solution.converged);
        assert!(results[1].smoothed_pseudoranges.is_none());
        assert!(results[1].smoothed_solution.is_none());
        assert!(results[2].raw_solution.converged);
        let reset_smoothed = results[2].smoothed_pseudoranges.as_ref().unwrap();
        assert_eq!(
            reset_smoothed[0].pseudorange_m,
            epochs[2].pseudoranges[0].pseudorange_m
        );
    }

    #[test]
    fn test_continuous_runner_rejects_smoothed_solution_with_worse_residuals() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            hatch: Some(HatchConfig {
                window_epochs: 10,
                max_gap_s: 2.0,
                divergence_reset_threshold_m: 10_000.0,
                max_smoothed_residual_rms_ratio: 1.0,
                max_smoothed_residual_rms_margin_m: 0.0,
                reset_on_rejected_solution: false,
            }),
            ..Default::default()
        });
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: four_prns(20_000_000.0, 100.0),
        }];

        let mut calls = 0usize;
        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |_| {
                calls += 1;
                let residual = if calls == 1 { 1.0 } else { 10.0 };
                solved_with_residuals(4, vec![residual; 4])
            },
            |_, _| Some(velocity_solution(4, 1.0)),
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].raw_solution.converged);
        assert!(results[0].smoothed_pseudoranges.is_some());
        assert!(results[0].smoothed_solution.is_none());
        assert!(results[0].smoothed_candidate_solution.is_some());
        assert_eq!(
            results[0].smoothed_candidate_disposition,
            PvtCandidateDisposition::QualityRejected
        );
        assert!(results[0].smoothed_velocity_solution.is_none());
        let output = results[0].to_output();
        assert!(output.candidates.iter().any(|candidate| {
            candidate.solution_kind == PvtEpochSolutionKind::Smoothed && candidate.quality.converged
        }));
        assert!(output.candidate_statuses.iter().any(|status| {
            status.solution_kind == PvtEpochSolutionKind::Smoothed
                && status.disposition == PvtCandidateDisposition::QualityRejected
        }));
    }

    #[test]
    fn test_continuous_runner_keeps_hatch_state_after_rejected_candidate_by_default() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            hatch: Some(HatchConfig {
                window_epochs: 10,
                max_gap_s: 2.0,
                divergence_reset_threshold_m: 10_000.0,
                max_smoothed_residual_rms_ratio: 1.0,
                max_smoothed_residual_rms_margin_m: 0.0,
                reset_on_rejected_solution: false,
            }),
            ..Default::default()
        });
        let epochs = vec![
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 100.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_100.0, 101.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_200.0, 102.0),
            },
        ];

        let mut calls = 0usize;
        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |_| {
                calls += 1;
                let residual = if calls == 4 { 10.0 } else { 1.0 };
                solved_with_residuals(4, vec![residual; 4])
            },
            |_, _| Some(velocity_solution(4, 1.0)),
        );

        assert!(results[1].smoothed_solution.is_none());
        let third_smoothed = results[2].smoothed_pseudoranges.as_ref().unwrap();
        assert!(
            third_smoothed[0].pseudorange_m < epochs[2].pseudoranges[0].pseudorange_m,
            "Hatch state should survive a rejected smoothed candidate"
        );
    }

    #[test]
    fn test_continuous_runner_updates_kalman_from_velocity_epochs() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            kalman: Some(KalmanConfig::default()),
            ..Default::default()
        });
        let epochs = vec![
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_000.0, 100.0),
            },
            PseudorangeEpoch {
                pseudoranges: four_prns(20_000_100.0, 101.0),
            },
        ];

        let mut solve_calls = 0usize;
        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |pseudoranges| {
                solve_calls += 1;
                let mut solution = solved(pseudoranges.len());
                solution.ecef = [solve_calls as f64, 0.0, 0.0];
                solution
            },
            |_, _| Some(velocity_solution(4, 1.0)),
        );

        assert_eq!(results.len(), 2);
        assert!(results[0]
            .kalman_solution
            .as_ref()
            .is_some_and(|s| s.converged));
        assert!(results[1]
            .kalman_solution
            .as_ref()
            .is_some_and(|s| s.converged));
        assert!(
            results[1].kalman_solution.as_ref().unwrap().ecef[0]
                > results[0].kalman_solution.as_ref().unwrap().ecef[0]
        );
    }

    #[test]
    fn test_epoch_output_prefers_kalman_and_exposes_velocity_time_quality() {
        let mut runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
            kalman: Some(KalmanConfig::default()),
            ..Default::default()
        });
        let epochs = vec![PseudorangeEpoch {
            pseudoranges: four_prns(20_000_000.0, 100.0),
        }];

        let results = runner.solve_epochs_with_velocity(
            &epochs,
            |pseudoranges| {
                let mut solution = solved(pseudoranges.len());
                solution.clock_bias_s = 2e-6;
                solution.n_sats = pseudoranges.len();
                solution.used_prns = pseudoranges.iter().map(|pr| pr.prn).collect();
                solution.residual_rms_m = 1.25;
                solution.satellite_diagnostics = vec![PvtSatelliteDiagnostic {
                    prn: 1,
                    azimuth_rad: 1.25,
                    elevation_rad: 0.75,
                    residual_m: -0.4,
                    standardized_residual: -0.2,
                    pseudorange_sigma_m: 2.0,
                    cn0_sigma_m: 0.0,
                    elevation_sigma_m: 0.0,
                    iono_sigma_m: 0.0,
                    tracking_jitter_sigma_m: 0.0,
                    weight: 0.25,
                    iono_delay_m: 0.0,
                    tropo_delay_m: 0.0,
                }];
                solution
            },
            |_, _| {
                Some(VelocitySolution {
                    ecef_velocity_mps: [3.0, 4.0, 0.0],
                    enu_velocity_mps: [3.0, 4.0, 0.0],
                    clock_drift_mps: 6.0,
                    clock_drift_sps: 6.0 / C_LIGHT,
                    residuals_mps: vec![0.1, -0.2, 0.2, -0.1],
                    used_prns: vec![1, 2, 3, 4],
                    converged: true,
                })
            },
        );

        let output = results[0].to_output();

        assert_eq!(output.solution_kind, PvtEpochSolutionKind::Kalman);
        assert_eq!(
            output
                .candidates
                .iter()
                .map(|candidate| candidate.solution_kind)
                .collect::<Vec<_>>(),
            vec![PvtEpochSolutionKind::Raw, PvtEpochSolutionKind::Kalman]
        );
        assert_eq!(output.time.receiver_time_sow, Some(100.0));
        assert!((output.time.corrected_gps_time_sow.unwrap() - 99.999_998).abs() < 1e-12);
        assert_eq!(output.quality.n_sats, 4);
        assert_eq!(output.quality.residual_rms_m, 1.25);
        let velocity = output.velocity.as_ref().unwrap();
        assert!((velocity.speed_mps - 5.0).abs() < 1e-12);
        assert!((velocity.horizontal_speed_mps - 5.0).abs() < 1e-12);
        assert!((velocity.clock_drift_sps - 6.0 / C_LIGHT).abs() < 1e-15);
        assert_eq!(output.satellites.len(), 1);
        let satellite = &output.satellites[0];
        assert_eq!(satellite.prn, 1);
        assert_eq!(satellite.azimuth_rad, 1.25);
        assert_eq!(satellite.elevation_rad, 0.75);
        assert_eq!(satellite.residual_m, -0.4);
        assert!(satellite.used_in_solution);
        assert!(!satellite.raim_excluded);
    }

    fn four_prns(base_pseudorange_m: f64, receive_time_sow: f64) -> Vec<Pseudorange> {
        (1..=4)
            .map(|prn| {
                pseudorange(
                    prn,
                    base_pseudorange_m + prn as f64 * 1_000.0,
                    receive_time_sow,
                )
            })
            .collect()
    }

    fn pseudorange(prn: u8, pseudorange_m: f64, receive_time_sow: f64) -> Pseudorange {
        Pseudorange {
            prn,
            pseudorange_m,
            transmit_time_sow: receive_time_sow - 0.075,
            receive_time_sow,
            cn0_db_hz: 45.0,
            doppler_hz: -500.0,
            tracking_jitter_chips: 0.0,
        }
    }

    fn solved(n_sats: usize) -> PvtSolution {
        PvtSolution {
            n_sats,
            converged: true,
            ..Default::default()
        }
    }

    fn solved_with_residuals(n_sats: usize, residuals_m: Vec<f64>) -> PvtSolution {
        PvtSolution {
            n_sats,
            residuals_m,
            converged: true,
            ..Default::default()
        }
    }

    fn velocity_solution(n_sats: usize, vx_mps: f64) -> VelocitySolution {
        VelocitySolution {
            ecef_velocity_mps: [vx_mps, 0.0, 0.0],
            enu_velocity_mps: [0.0; 3],
            used_prns: (1..=n_sats as u8).collect(),
            converged: true,
            ..Default::default()
        }
    }

    fn ephemeris_for_catalog_test(prn: u8, toe: f64, iode: u8) -> crate::nav::Ephemeris {
        crate::nav::Ephemeris {
            prn,
            toe,
            toc: toe,
            iode,
            sqrt_a: 26_559_710.0_f64.sqrt(),
            eccentricity: 0.01,
            ..Default::default()
        }
    }

    fn kalman_speed_mps(solution: &KalmanSolution) -> f64 {
        (solution.ecef_velocity_mps[0].powi(2)
            + solution.ecef_velocity_mps[1].powi(2)
            + solution.ecef_velocity_mps[2].powi(2))
        .sqrt()
    }
}
