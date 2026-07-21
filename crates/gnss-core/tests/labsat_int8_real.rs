use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::pvt::IntegerMsAmbiguityConfig;
use gnss_core::receiver::{StreamingPvtConfig, StreamingPvtEvent, StreamingPvtRunner};
use gnss_data::gps_l1ca::PRN_COUNT;
use num_complex::Complex;
use std::collections::BTreeMap;
use std::path::Path;

const DATA_FILE: &str = "../../data/202306202155/Data202306202155(2).dat";
const SAMPLE_RATE_HZ: f64 = 10_230_000.0;
const ACQUISITION_SAMPLE_RATE_HZ: f64 = SAMPLE_RATE_HZ / 5.0;
const EXPECTED_FILE_BYTES: u64 = 2_670_030_000;

fn decimate_by_5(samples: &[Complex<f64>]) -> Vec<Complex<f64>> {
    samples
        .chunks_exact(5)
        .map(|chunk| chunk.iter().copied().sum::<Complex<f64>>() / 5.0)
        .collect()
}

#[test]
fn labsat_int8_iq_acquires_real_gps_l1_ca_signals() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: LabSat Int8 real IQ data not present");
        return;
    }

    let mut reader = IqReader::open(data_path, IqFormat::Int8).expect("open LabSat Int8 IQ");
    assert_eq!(
        std::fs::metadata(data_path)
            .expect("read LabSat Int8 IQ metadata")
            .len(),
        EXPECTED_FILE_BYTES
    );
    assert_eq!(reader.total_samples(), EXPECTED_FILE_BYTES / 2);

    let samples_per_ms = (SAMPLE_RATE_HZ / 1_000.0) as usize;
    let integration_ms = 10;
    let sample_count = samples_per_ms * integration_ms;
    let mut samples = vec![Complex::new(0.0, 0.0); sample_count];
    assert_eq!(
        reader
            .read(&mut samples, sample_count)
            .expect("read LabSat Int8 IQ"),
        sample_count
    );
    let samples = decimate_by_5(&samples);

    let config = PcpsConfig {
        fs: ACQUISITION_SAMPLE_RATE_HZ,
        if_freq: 0.0,
        doppler_max: 10_000.0,
        doppler_step: 500.0,
        coherent_integration_ms: 1.0,
        noncoherent_integrations: integration_ms,
        threshold: 2.2,
    };
    let sample_window = samples.as_slice();
    let mut results = std::thread::scope(|scope| {
        (0..PRN_COUNT)
            .map(|prn| {
                scope.spawn(move || {
                    let mut acquisition = PcpsAcquisition::new(config);
                    acquisition.acquire(prn, sample_window)
                })
            })
            .map(|handle| {
                handle
                    .join()
                    .expect("PCPS acquisition worker should not panic")
            })
            .collect::<Vec<_>>()
    });
    results.sort_by(|left, right| right.peak_to_mean.total_cmp(&left.peak_to_mean));

    eprintln!("LabSat Int8 acquisition candidates:");
    for result in results.iter().take(12) {
        eprintln!(
            "  G{:02}: metric={:.3}, doppler={:.0} Hz, code={:.3} chips, detected={}",
            result.prn,
            result.peak_to_mean,
            result.doppler_hz,
            result.code_phase_chips,
            result.detected
        );
    }

    let detections = results
        .iter()
        .filter(|result| result.detected)
        .collect::<Vec<_>>();
    assert!(
        detections.len() >= 4,
        "expected at least four GPS L1 C/A detections, got {}",
        detections.len()
    );
    for expected_prn in [10, 12, 23, 28] {
        assert!(
            detections.iter().any(|result| result.prn == expected_prn),
            "expected stable LabSat detection G{expected_prn:02}"
        );
    }
}

#[test]
#[ignore = "processes up to 90 seconds of real LabSat IQ in release mode"]
fn labsat_int8_iq_produces_pvt() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: LabSat Int8 real IQ data not present");
        return;
    }

    let fixed_prns = vec![10, 12, 23, 24, 25, 28, 32];
    let mut config = StreamingPvtConfig::cttc_fixed_prns(fixed_prns.clone());
    config.iq_path = Some(data_path.to_path_buf());
    config.iq_format = IqFormat::Int8;
    config.sample_rate_hz = SAMPLE_RATE_HZ;
    config.intermediate_frequency_hz = 0.0;
    config.chunk_ms = 20;
    config.pvt_output_rate_ms = 1_000;
    config.acquisition.acq_window_ms = 10;
    config.acquisition.noncoherent_integrations = 10;
    config.acquisition.cold_acq_interval_ms = 1;
    config.acquisition.reacq_interval_ms = 1_000;
    config.acquisition.max_acq_prns_per_scan = fixed_prns.len();
    config.acquisition.doppler_step_hz = 250.0;
    config.acquisition.threshold = 3.0;
    config.acquisition_ring_ms = 10;
    config.continuous_pvt.integer_ms_ambiguity = Some(IntegerMsAmbiguityConfig::default());
    config.continuous_pvt.initial_position_lla_rad =
        Some([40.00154_f64.to_radians(), 116.33028_f64.to_radians(), 95.0]);
    config.continuous_pvt.gps_week_rollover_reference = Some(2_267);
    config.continuous_pvt.wls.raim_max_exclusions = 0;

    let max_ms = std::env::var("GNSSCOPE_LABSAT_TEST_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(90_000);
    let mut outputs = Vec::new();
    let mut latest_tracking = BTreeMap::new();
    let mut runner = StreamingPvtRunner::new(config);
    let summary = runner
        .stream_iq_file_until_ms(max_ms, |step| {
            for event in step.events {
                match event {
                    StreamingPvtEvent::PvtOutput(output) => outputs.push(*output),
                    StreamingPvtEvent::TrackingBatch(updates) => {
                        for update in updates {
                            latest_tracking.insert(
                                update.prn,
                                (update.output.cn0_db_hz, update.output.pll_lock_metric),
                            );
                        }
                    }
                    _ => {}
                }
            }
        })
        .expect("stream LabSat Int8 IQ through PVT");

    eprintln!(
        "LabSat active tracking PRNs: {:?}",
        runner.active_tracking_prns()
    );
    for channel in runner.nav_state().channels.values() {
        eprintln!(
            "  G{:02}: subframes={}, last_tow={:?}",
            channel.prn, channel.subframes_decoded, channel.last_tow_count
        );
    }
    for (prn, (cn0_db_hz, pll_lock_metric)) in &latest_tracking {
        eprintln!("  G{prn:02}: C/N0={cn0_db_hz:.2} dB-Hz, PLL lock={pll_lock_metric:.3}");
    }
    eprintln!(
        "LabSat events: acquisition={}, tracking={}, bit_sync={}, nav_symbols={}, nav_subframes={}, pvt={}",
        summary.event_counts.acquisition_scans_completed,
        summary.event_counts.tracking_updates,
        summary.event_counts.bit_sync_updates,
        summary.event_counts.nav_symbols,
        summary.event_counts.nav_subframes,
        summary.event_counts.pvt_outputs
    );

    let converged = outputs
        .iter()
        .filter(|output| output.quality.converged)
        .collect::<Vec<_>>();
    let last = converged.last().expect("converged LabSat PVT output");
    let velocity = last.velocity.as_ref().expect("LabSat velocity output");
    let latitude_deg = last.lla_rad[0].to_degrees();
    let longitude_deg = last.lla_rad[1].to_degrees();
    eprintln!(
        "LabSat final PVT: epoch={}, lat={:.8}, lon={:.8}, alt={:.3} m, speed={:.4} m/s, clock_drift={:.4} m/s, sats={:?}",
        last.epoch_index,
        latitude_deg,
        longitude_deg,
        last.lla_rad[2],
        velocity.speed_mps,
        velocity.clock_drift_mps,
        last.quality.used_prns
    );
    eprintln!("LabSat final time: {:?}", last.time.utc);
    assert!((latitude_deg - 40.00154).abs() < 0.001);
    assert!((longitude_deg - 116.33028).abs() < 0.001);
    assert!((last.lla_rad[2] - 95.0).abs() < 50.0);
    assert!(velocity.speed_mps < 1.0);
    let utc = last.time.utc.expect("LabSat UTC output");
    assert_eq!((utc.year, utc.month, utc.day), (2023, 6, 20));
    assert_eq!((utc.hour, utc.minute), (13, 55));
    assert!(summary.event_counts.nav_subframes > 0);
    assert!(summary.event_counts.pvt_outputs > 0);
}

#[test]
fn labsat_int8_iq_enters_streaming_tracking() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: LabSat Int8 real IQ data not present");
        return;
    }

    let mut config = StreamingPvtConfig::cttc_fixed_prns(vec![10]);
    config.iq_path = Some(data_path.to_path_buf());
    config.iq_format = IqFormat::Int8;
    config.sample_rate_hz = SAMPLE_RATE_HZ;
    config.intermediate_frequency_hz = 0.0;
    config.chunk_ms = 10;
    config.acquisition.acq_window_ms = 10;
    config.acquisition.noncoherent_integrations = 10;
    config.acquisition.cold_acq_interval_ms = 1;
    config.acquisition.doppler_step_hz = 250.0;
    config.acquisition.threshold = 3.0;
    config.acquisition_ring_ms = 10;
    config.channel_admission.probation_good_epochs = 1;
    config.channel_admission.min_cn0_db_hz = 0.0;
    config.channel_admission.min_pll_lock_metric = 0.0;

    let mut acquisition_detected = false;
    let mut tracking_updated = false;
    let mut runner = StreamingPvtRunner::new(config);
    runner
        .stream_iq_file_until_ms(25, |step| {
            for event in step.events {
                match event {
                    StreamingPvtEvent::AcquisitionScanCompleted(scan) => {
                        acquisition_detected |= scan
                            .results
                            .iter()
                            .any(|result| result.prn == 10 && result.detected);
                    }
                    StreamingPvtEvent::TrackingBatch(updates) => {
                        tracking_updated |= updates.iter().any(|update| update.prn == 10);
                    }
                    _ => {}
                }
            }
        })
        .expect("stream LabSat Int8 IQ");

    assert!(acquisition_detected, "streaming runner did not acquire G10");
    assert!(tracking_updated, "streaming runner did not track G10");
    assert_eq!(runner.active_tracking_prns(), vec![10]);
}
