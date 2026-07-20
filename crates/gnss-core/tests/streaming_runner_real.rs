//! Streaming runner smoke test on real CTTC Spain data.
//!
//! The default smoke duration is short. `GNSSCOPE_STREAMING_TEST_MS` extends it
//! into a production-path throughput and navigation/PVT regression run.

use gnss_core::io::{IqFormat, IqReader};
use gnss_core::pvt::PositionQualityConfig;
use gnss_core::receiver::{
    AcquisitionScanKind, ChannelHealthObservation, ReceiverChannelState, StreamingPvtConfig,
    StreamingPvtEvent, StreamingPvtRunner,
};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

const DATA_FILE: &str =
    "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";

fn mean_and_population_sigma(values: &[f64]) -> (f64, f64) {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let sigma = (values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64)
        .sqrt();
    (mean, sigma)
}

fn moving_average(values: &[f64], depth: usize) -> Vec<f64> {
    values
        .windows(depth)
        .map(|window| window.iter().sum::<f64>() / depth as f64)
        .collect()
}

#[test]
fn test_streaming_runner_real_data_smoke() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real data not present");
        return;
    }

    let max_ms = std::env::var("GNSSCOPE_STREAMING_TEST_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(25);
    let parallel_tracking = std::env::var("GNSSCOPE_PARALLEL_TRACKING")
        .ok()
        .is_none_or(|value| value != "0");
    let mut config = StreamingPvtConfig::cttc_spain_real_data(DATA_FILE);
    config.parallel_tracking = parallel_tracking;
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

    let started = Instant::now();
    let position_quality = PositionQualityConfig::default();
    let mut implausible_position_epochs = Vec::new();
    let mut converged_satellite_counts = Vec::new();
    let mut converged_altitudes_m = Vec::new();
    let mut first_converged_epoch_index = None;
    let mut unconverged_after_first_solution = Vec::new();
    let summary = runner
        .stream_iq_file_until_ms(max_ms, |step| {
            implausible_position_epochs.extend(step.events.iter().filter_map(|event| {
                let StreamingPvtEvent::PvtOutput(output) = event else {
                    return None;
                };
                if output.quality.converged {
                    first_converged_epoch_index.get_or_insert(output.epoch_index);
                    converged_satellite_counts.push(output.quality.n_sats);
                    converged_altitudes_m.push(output.lla_rad[2]);
                } else if first_converged_epoch_index.is_some() {
                    unconverged_after_first_solution.push((
                        output.epoch_index,
                        output.quality.n_sats,
                        output.time.receiver_time_sow,
                    ));
                }
                (output.quality.converged
                    && !(position_quality.min_altitude_m..=position_quality.max_altitude_m)
                        .contains(&output.lla_rad[2]))
                .then_some((output.epoch_index, output.lla_rad[2]))
            }));
        })
        .expect("stream real CTTC IQ");
    let elapsed = started.elapsed();
    eprintln!(
        "streamed {max_ms} ms in {:.3} s ({:.2}x real time), parallel_tracking={parallel_tracking}",
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / max_ms as f64,
    );
    eprintln!("event counts: {:?}", summary.event_counts);
    if let Some(output) = summary.latest_pvt_output.as_ref() {
        eprintln!(
            "latest PVT time: GPS week {:?}, SOW {:?}, UTC {:?}",
            output.time.gps_week, output.time.corrected_gps_time_sow, output.time.utc
        );
    }

    assert_eq!(summary.samples_read, max_ms * 4_000);
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
    assert!(
        implausible_position_epochs.is_empty(),
        "converged PVT outputs outside the terrestrial altitude gate: {implausible_position_epochs:?}"
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

    if max_ms >= 60_000 {
        assert!(summary.event_counts.nav_subframes > 0);
        assert!(summary.event_counts.pvt_outputs > 0);
        assert!(
            unconverged_after_first_solution.is_empty(),
            "PVT continuity gaps after first solution: {unconverged_after_first_solution:?}"
        );
        let first_converged_epoch_index =
            first_converged_epoch_index.expect("at least one converged CTTC PVT output");
        eprintln!("first converged CTTC PVT epoch: {first_converged_epoch_index}");
        assert!(
            first_converged_epoch_index < 100,
            "first converged CTTC PVT output arrived too late: {first_converged_epoch_index}"
        );
        let time = &summary
            .latest_pvt_output
            .expect("latest streaming PVT")
            .time;
        assert_eq!(time.gps_week, Some(1734));
        let five_satellite_fraction = converged_satellite_counts
            .iter()
            .filter(|count| **count == 5)
            .count() as f64
            / converged_satellite_counts.len() as f64;
        assert!(
            five_satellite_fraction >= 0.99,
            "expected at least 99% five-satellite converged CTTC solutions, got {:.2}%",
            five_satellite_fraction * 100.0
        );
        let (altitude_mean_m, altitude_sigma_m) = mean_and_population_sigma(&converged_altitudes_m);
        let averaged_altitudes_m = moving_average(&converged_altitudes_m, 10);
        let (averaged_altitude_mean_m, averaged_altitude_sigma_m) =
            mean_and_population_sigma(&averaged_altitudes_m);
        eprintln!(
            "CTTC altitude: raw n={} mean={altitude_mean_m:.3} m sigma={altitude_sigma_m:.3} m; 10-point n={} mean={averaged_altitude_mean_m:.3} m sigma={averaged_altitude_sigma_m:.3} m; five-satellite={:.2}%",
            converged_altitudes_m.len(),
            averaged_altitudes_m.len(),
            five_satellite_fraction * 100.0,
        );
        assert!(
            altitude_sigma_m < 20.0,
            "raw CTTC altitude sigma unexpectedly large: {altitude_sigma_m:.3} m"
        );
        assert!(
            averaged_altitude_sigma_m < altitude_sigma_m,
            "10-point averaging should reduce CTTC altitude sigma: raw={altitude_sigma_m:.3} m averaged={averaged_altitude_sigma_m:.3} m"
        );
        let utc = time.utc.expect("streaming UTC time");
        assert_eq!((utc.year, utc.month, utc.day), (2013, 4, 4));
        if max_ms == 60_000 {
            assert_eq!((utc.hour, utc.minute), (6, 24));
            assert!(
                (utc.second - 7.029).abs() < 0.1,
                "UTC second: {}",
                utc.second
            );
        }
    }
}

#[test]
fn test_streaming_runner_real_data_reacquires_forced_lost_channel() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real data not present");
        return;
    }

    let mut config = StreamingPvtConfig::cttc_spain_real_data(DATA_FILE);
    config.acquisition.reacq_interval_ms = 1;
    config.channel_admission.probation_good_epochs = 1;
    config.channel_admission.min_cn0_db_hz = 0.0;
    config.channel_admission.min_pll_lock_metric = 0.0;
    let mut runner = StreamingPvtRunner::new(config);
    let mut reader = IqReader::open(data_path, IqFormat::Int16).expect("open real CTTC IQ");

    runner
        .stream_iq_reader_until_ms(&mut reader, 25, |_| {})
        .expect("establish real-data tracking");
    let prn = *runner
        .active_tracking_prns()
        .first()
        .expect("at least one tracked real-data PRN");
    for now_ms in 25..35 {
        runner.observe_tracking(
            prn,
            ChannelHealthObservation {
                pll_lock_metric: 0.0,
                cn0_db_hz: 0.0,
                dll_error_chips: 10.0,
                pll_error_cycles: 1.0,
                doppler_hz: 0.0,
                nav_time_available: false,
                ephemeris_available: false,
            },
            now_ms,
        );
    }
    assert_eq!(runner.channels().state(prn), ReceiverChannelState::Lost);

    let mut reacquisition_detected = false;
    let mut tracking_restored = false;
    runner
        .stream_iq_reader_until_ms(&mut reader, 100, |step| {
            for event in step.events {
                match event {
                    StreamingPvtEvent::AcquisitionScanCompleted(scan)
                        if scan.scan.plan.kind == AcquisitionScanKind::Reacquisition =>
                    {
                        reacquisition_detected |= scan
                            .results
                            .iter()
                            .any(|result| result.prn == prn && result.detected);
                    }
                    StreamingPvtEvent::TrackingBatch(updates) => {
                        tracking_restored |= updates.iter().any(|update| {
                            update.prn == prn
                                && matches!(
                                    update.state,
                                    ReceiverChannelState::TrackingProbation
                                        | ReceiverChannelState::Healthy
                                )
                        });
                    }
                    _ => {}
                }
            }
        })
        .expect("continue real-data stream after forced loss");

    assert!(
        reacquisition_detected,
        "forced-lost PRN {prn} was not reacquired"
    );
    assert!(
        tracking_restored,
        "reacquired PRN {prn} did not resume tracking"
    );
}

#[test]
#[ignore = "requires 28 seconds of real CTTC IQ; run in release mode"]
fn test_streaming_runner_real_data_resumes_pvt_after_multi_channel_reacquisition() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real data not present");
        return;
    }

    let mut config = StreamingPvtConfig::cttc_spain_real_data(DATA_FILE);
    config.acquisition.reacq_interval_ms = 500;
    config.acquisition.max_acq_prns_per_scan = 1;
    config.channel_admission.probation_good_epochs = 1;
    config.channel_admission.min_cn0_db_hz = 0.0;
    config.channel_admission.min_pll_lock_metric = 0.0;
    let samples_per_ms = config.samples_per_ms() as u64;
    let mut runner = StreamingPvtRunner::new(config);
    let mut reader = IqReader::open(data_path, IqFormat::Int16).expect("open real CTTC IQ");

    let established = runner
        .stream_iq_reader_until_ms(&mut reader, 27_000, |_| {})
        .expect("establish converged real-data PVT");
    let pre_loss_pvt = established
        .latest_pvt_output
        .expect("converged PVT before forced multi-channel loss");
    assert!(
        pre_loss_pvt.quality.converged,
        "latest pre-loss PVT must be converged"
    );

    let eligible_prns = runner.channels().pvt_eligible_prns();
    assert_eq!(
        eligible_prns.len(),
        5,
        "CTTC recovery regression requires all five channels healthy before injection: {eligible_prns:?}"
    );
    let forced_lost_prns = eligible_prns.into_iter().take(3).collect::<Vec<_>>();
    let loss_now_ms = runner.state_snapshot().sample_index / samples_per_ms;
    let bad_observation = ChannelHealthObservation {
        pll_lock_metric: 0.0,
        cn0_db_hz: 0.0,
        dll_error_chips: 10.0,
        pll_error_cycles: 1.0,
        doppler_hz: 0.0,
        nav_time_available: false,
        ephemeris_available: false,
    };
    for &prn in &forced_lost_prns {
        for offset_ms in 0..10 {
            runner.observe_tracking(prn, bad_observation, loss_now_ms + offset_ms);
        }
        assert_eq!(
            runner.channels().state(prn),
            ReceiverChannelState::Lost,
            "PRN {prn} did not enter Lost after forced health failures"
        );
    }
    assert_eq!(runner.channels().pvt_eligible_prns().len(), 2);

    let mut reacquired_prns = BTreeSet::new();
    let mut restored_tracking_prns = BTreeSet::new();
    let mut degraded_pvt_outputs = Vec::new();
    let degraded = runner
        .stream_iq_reader_until_ms(&mut reader, 300, |step| {
            for event in step.events {
                match event {
                    StreamingPvtEvent::AcquisitionScanCompleted(scan)
                        if scan.scan.plan.kind == AcquisitionScanKind::Reacquisition =>
                    {
                        reacquired_prns.extend(scan.results.iter().filter_map(|result| {
                            (result.detected && forced_lost_prns.contains(&result.prn))
                                .then_some(result.prn)
                        }));
                    }
                    StreamingPvtEvent::TrackingBatch(updates) => {
                        restored_tracking_prns.extend(updates.iter().filter_map(|update| {
                            (forced_lost_prns.contains(&update.prn)
                                && matches!(
                                    update.state,
                                    ReceiverChannelState::TrackingProbation
                                        | ReceiverChannelState::Healthy
                                ))
                            .then_some(update.prn)
                        }));
                    }
                    StreamingPvtEvent::PvtOutput(output) => {
                        degraded_pvt_outputs.push(*output);
                    }
                    _ => {}
                }
            }
        })
        .expect("stream forced PVT interruption window");
    let degraded_eligible_count = degraded
        .final_state
        .expect("degraded final state")
        .channels
        .iter()
        .filter(|channel| channel.pvt_eligible)
        .count();
    assert!(
        degraded_eligible_count < 4,
        "PVT interruption window unexpectedly retained {degraded_eligible_count} eligible channels"
    );
    assert_eq!(
        degraded_pvt_outputs.len(),
        3,
        "300 ms interruption window must retain the configured 100 ms PVT cadence"
    );
    assert!(
        degraded_pvt_outputs
            .iter()
            .all(|output| !output.quality.converged),
        "PVT converged with fewer than four eligible channels: {:?}",
        degraded_pvt_outputs
            .iter()
            .map(|output| (output.epoch_index, output.quality.n_sats))
            .collect::<Vec<_>>()
    );

    let mut recovered_pvt_outputs = Vec::new();
    runner
        .stream_iq_reader_until_ms(&mut reader, 1_200, |step| {
            for event in step.events {
                match event {
                    StreamingPvtEvent::AcquisitionScanCompleted(scan)
                        if scan.scan.plan.kind == AcquisitionScanKind::Reacquisition =>
                    {
                        reacquired_prns.extend(scan.results.iter().filter_map(|result| {
                            (result.detected && forced_lost_prns.contains(&result.prn))
                                .then_some(result.prn)
                        }));
                    }
                    StreamingPvtEvent::TrackingBatch(updates) => {
                        restored_tracking_prns.extend(updates.iter().filter_map(|update| {
                            (forced_lost_prns.contains(&update.prn)
                                && matches!(
                                    update.state,
                                    ReceiverChannelState::TrackingProbation
                                        | ReceiverChannelState::Healthy
                                ))
                            .then_some(update.prn)
                        }));
                    }
                    StreamingPvtEvent::PvtOutput(output) => {
                        recovered_pvt_outputs.push(*output);
                    }
                    _ => {}
                }
            }
        })
        .expect("stream real-data PVT recovery window");

    assert!(
        reacquired_prns.len() >= 2,
        "fewer than two forced-lost PRNs were reacquired: forced={forced_lost_prns:?} reacquired={reacquired_prns:?}"
    );
    assert!(
        restored_tracking_prns.len() >= 2,
        "fewer than two reacquired PRNs resumed tracking: {restored_tracking_prns:?}"
    );
    let recovered_pvt = recovered_pvt_outputs
        .iter()
        .find(|output| output.quality.converged)
        .expect("converged PVT must resume after reacquisition");
    assert!(
        recovered_pvt.epoch_index > pre_loss_pvt.epoch_index,
        "recovered PVT epoch {} did not advance beyond pre-loss epoch {}",
        recovered_pvt.epoch_index,
        pre_loss_pvt.epoch_index
    );
    assert!(
        recovered_pvt.quality.n_sats >= 4,
        "recovered PVT used only {} satellites",
        recovered_pvt.quality.n_sats
    );
    eprintln!(
        "PVT recovery: pre-loss epoch={}, forced={forced_lost_prns:?}, reacquired={reacquired_prns:?}, restored={restored_tracking_prns:?}, degraded_outputs={}, recovered_epoch={}, recovered_sats={}",
        pre_loss_pvt.epoch_index,
        degraded_pvt_outputs.len(),
        recovered_pvt.epoch_index,
        recovered_pvt.quality.n_sats,
    );
}
