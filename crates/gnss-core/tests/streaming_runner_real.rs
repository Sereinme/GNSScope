//! Streaming runner smoke test on real CTTC Spain data.
//!
//! This intentionally stops early: it verifies that the production streaming
//! entry point can load optional colocated RINEX nav, acquire the fixed CTTC
//! PRNs, and advance tracking from the IQ reader without the legacy e2e test
//! hand-driving those stages.

use gnss_core::receiver::{ReceiverChannelState, StreamingPvtConfig, StreamingPvtRunner};
use std::path::Path;

const DATA_FILE: &str =
    "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";

#[test]
fn test_streaming_runner_real_data_smoke() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real data not present");
        return;
    }

    let mut config = StreamingPvtConfig::cttc_spain_real_data(DATA_FILE);
    config.channel_admission.probation_good_epochs = 1;
    config.channel_admission.min_cn0_db_hz = 0.0;
    config.channel_admission.min_pll_lock_metric = 0.0;

    let mut runner = StreamingPvtRunner::new(config);
    assert!(
        runner.nav_state().rinex_nav_source.is_some(),
        "colocated RINEX nav should be loaded when present"
    );
    assert!(
        !runner.nav_state().ephemeris_catalog.is_empty(),
        "RINEX ephemeris catalog should be available for streaming PVT"
    );

    let summary = runner
        .stream_iq_file_until_ms(25, |_| {})
        .expect("stream real CTTC IQ");

    assert_eq!(summary.samples_read, 100_000);
    assert!(summary.event_counts.acquisition_scans_completed >= 1);
    assert!(
        summary.event_counts.acquisition_detections >= 4,
        "expected most CTTC PRNs to acquire, got {}",
        summary.event_counts.acquisition_detections
    );
    assert!(
        summary.event_counts.tracking_updates >= summary.event_counts.acquisition_detections,
        "tracking should advance for acquired channels"
    );

    let final_state = summary.final_state.expect("streaming final state");
    let active_count = final_state
        .channels
        .iter()
        .filter(|channel| channel.tracking_active)
        .count();
    assert!(
        active_count >= 4,
        "active tracking channels: {active_count}"
    );
    assert!(final_state.channels.iter().any(|channel| {
        matches!(
            channel.state,
            ReceiverChannelState::TrackingProbation
                | ReceiverChannelState::Healthy
                | ReceiverChannelState::Suspect
        )
    }));
}
