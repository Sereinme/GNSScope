//! PVT (Position, Velocity, Time) solving.
//!
//! Pipeline: ephemeris parsing → satellite position → atmospheric corrections →
//! pseudorange → WLS solver → ECEF/LLA coordinates.

pub mod atmosphere;
pub mod continuous;
pub mod coords;
pub mod kalman;
pub mod pseudorange;
pub mod satpos;
pub mod smoothing;
pub mod solver;
pub mod time;
pub mod velocity;

pub use continuous::{
    ContinuousPvtConfig, ContinuousPvtEpochResult, ContinuousPvtRunner, IntegerMsAmbiguityConfig,
    PositionQualityConfig, PseudorangeEpoch, PvtEpochOutput, PvtEpochSolutionKind,
    PvtQualityOutput, PvtSatelliteOutput, PvtTimeOutput, PvtVelocityOutput, VelocityQualityConfig,
};
pub use coords::{ecef_to_lla, lla_to_ecef, Ecef, Lla};
pub use kalman::{KalmanConfig, KalmanSolution, PositionVelocityKalman};
pub use pseudorange::{
    code_phase_samples_from_chips, from_common_receive_epoch,
    from_tow_history_common_receive_epoch, from_tracking_timestamp_common_receive_epoch,
    interpolate_tow_sow, pseudorange_delta_stats, resolve_integer_ms_ambiguities,
    ObservablesBuilder, ObservablesInputBuilder, ObservablesInputSet, ObservablesResult,
    ObservablesSource, Pseudorange, TowHistoryObservation, TowSample, TrackingChannelSnapshot,
    TrackingObservation, TrackingTimestampObservation,
};
pub use satpos::{satellite_position, satellite_velocity, SatPos};
pub use smoothing::{HatchConfig, HatchSmoother};
pub use solver::{
    solve_wls, solve_wls_with_config, PvtSatelliteDiagnostic, PvtSolution, WlsConfig,
};
pub use time::{gps_time_to_utc, normalize_gps_time, GpsTime, UtcDateTime};
pub use velocity::{
    doppler_velocity_observations, solve_doppler_velocity,
    solve_doppler_velocity_from_observations, DopplerVelocityObservation, VelocitySolution,
};
