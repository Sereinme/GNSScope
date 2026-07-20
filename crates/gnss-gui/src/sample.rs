use crate::GuiPvtViewModel;
use gnss_core::pvt::{
    PvtCandidateDisposition, PvtCandidateOutput, PvtCandidateStatusOutput, PvtEpochOutput,
    PvtEpochSolutionKind, PvtQualityOutput, PvtTimeOutput, PvtVelocityOutput, UtcDateTime,
};

pub fn sample_pvt_view_model() -> GuiPvtViewModel {
    GuiPvtViewModel::from_epoch_output(&sample_pvt_epoch_output())
}

fn sample_pvt_epoch_output() -> PvtEpochOutput {
    let velocity = PvtVelocityOutput {
        ecef_velocity_mps: [-2.631, 1.091, -0.289],
        enu_velocity_mps: [1.181, 1.493, -2.138],
        speed_mps: 2.863,
        horizontal_speed_mps: 1.904,
        vertical_speed_mps: -2.138,
        heading_rad: Some(1.181_f64.atan2(1.493)),
        clock_drift_mps: -1_582.812,
        clock_drift_sps: -5.28e-6,
        residual_rms_mps: 0.074,
        used_prns: vec![20, 1, 32, 11, 17],
    };
    let quality = PvtQualityOutput {
        converged: true,
        n_sats: 5,
        used_prns: vec![20, 1, 32, 11, 17],
        excluded_prns: Vec::new(),
        residual_rms_m: 0.08,
        standardized_residual_rms: 0.03,
        max_standardized_residual: 0.04,
        epoch_quality_scale: 1.0,
        hdop: 3.9,
        vdop: 4.5,
        pdop: 5.4,
        gdop: 5.94,
        horizontal_sigma_m: 18.0,
        vertical_sigma_m: 28.0,
        position_sigma_m: 22.0,
        velocity_sigma_mps: Some(0.10),
    };
    let ecef = [4_797_669.8, 166_502.1, 4_185_455.8];
    let lla_rad = [
        41.274_864_f64.to_radians(),
        1.987_639_f64.to_radians(),
        64.9,
    ];
    let candidates = [
        PvtEpochSolutionKind::Raw,
        PvtEpochSolutionKind::Smoothed,
        PvtEpochSolutionKind::Kalman,
    ]
    .into_iter()
    .map(|solution_kind| {
        let offset_m = match solution_kind {
            PvtEpochSolutionKind::Raw => 3.5,
            PvtEpochSolutionKind::Smoothed => 1.2,
            _ => 0.0,
        };
        let mut candidate_velocity = velocity.clone();
        candidate_velocity.speed_mps += offset_m * 0.01;
        candidate_velocity.horizontal_speed_mps += offset_m * 0.008;
        PvtCandidateOutput {
            solution_kind,
            ecef: [ecef[0] + offset_m, ecef[1], ecef[2]],
            lla_rad: [lla_rad[0], lla_rad[1], lla_rad[2] + offset_m],
            receiver_clock_bias_s: (7_755.461 + offset_m * 0.1) * 1e-6,
            velocity: Some(candidate_velocity),
            quality: quality.clone(),
            satellites: Vec::new(),
        }
    })
    .collect();
    let candidate_statuses = [
        PvtEpochSolutionKind::Raw,
        PvtEpochSolutionKind::Smoothed,
        PvtEpochSolutionKind::Kalman,
    ]
    .into_iter()
    .map(|solution_kind| PvtCandidateStatusOutput {
        solution_kind,
        disposition: if solution_kind == PvtEpochSolutionKind::Kalman {
            PvtCandidateDisposition::Selected
        } else {
            PvtCandidateDisposition::Available
        },
    })
    .collect();
    PvtEpochOutput {
        epoch_index: 34,
        solution_kind: PvtEpochSolutionKind::Kalman,
        time: PvtTimeOutput {
            gps_week: Some(1734),
            receiver_time_sow: Some(368_658.000),
            corrected_gps_time_sow: Some(368_657.992_245),
            utc_unix_seconds: Some(1_365_056_641.992_245),
            utc: Some(UtcDateTime {
                year: 2013,
                month: 4,
                day: 4,
                hour: 6,
                minute: 24,
                second: 1.992_245,
            }),
            transmit_time_sow_min: Some(368_657.918),
            transmit_time_sow_max: Some(368_657.925),
            receiver_clock_bias_s: 7_755.461e-6,
            receiver_clock_drift_sps: Some(-5.28e-6),
        },
        ecef,
        lla_rad,
        velocity: Some(velocity),
        quality,
        satellites: Vec::new(),
        candidates,
        candidate_statuses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gnss_core::pvt::PvtEpochSolutionKind;

    #[test]
    fn test_sample_pvt_view_model_is_gui_ready() {
        let mut sample = sample_pvt_view_model();

        assert_eq!(sample.solution_kind, PvtEpochSolutionKind::Kalman);
        assert!(sample.quality.converged);
        assert_eq!(sample.quality.n_sats, 5);
        assert!(sample
            .velocity
            .as_ref()
            .is_some_and(|velocity| velocity.speed_mps > 0.0));
        assert!(sample.time.corrected_gps_time_sow.is_some());
        assert_eq!(sample.candidates.len(), 3);
        assert_eq!(
            sample.candidate_disposition(PvtEpochSolutionKind::Kalman),
            PvtCandidateDisposition::Selected
        );
        sample.candidate_statuses.clear();
        assert_eq!(
            sample.candidate_disposition(PvtEpochSolutionKind::Raw),
            PvtCandidateDisposition::Available
        );
        assert_eq!(
            sample.candidate_disposition(PvtEpochSolutionKind::Kalman),
            PvtCandidateDisposition::Selected
        );
    }
}
