use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::nav::RinexNavConfig;
use gnss_core::pvt::{
    from_common_receive_epoch, from_tow_history_common_receive_epoch, ContinuousPvtConfig,
    ContinuousPvtRunner, IntegerMsAmbiguityConfig, ObservablesBuilder, PseudorangeEpoch,
};
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
        doppler_center_hz: 0.0,
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
    let diagnostic_rinex_path = std::env::var_os("GNSSCOPE_LABSAT_RINEX_NAV");
    if let Some(path) = diagnostic_rinex_path.as_ref() {
        config.rinex_nav = RinexNavConfig::with_explicit_path(path).without_environment();
    }

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

    if std::env::var_os("GNSSCOPE_LABSAT_DIAGNOSTICS").is_some() {
        if let Some(epoch) = runner.pseudorange_epoch_at_latest_sample() {
            let nav = runner.nav_state().clone();
            let base_config = runner.config().continuous_pvt.clone();
            let common_rx_sample = runner.state_snapshot().sample_index as f64;
            let observable_inputs = runner.observables_input(common_rx_sample);
            let observables = ObservablesBuilder::new(common_rx_sample, SAMPLE_RATE_HZ)
                .with_tracking_timestamp_observations(&observable_inputs.tracking_timestamp)
                .with_nco_reference_observations(&observable_inputs.nco_reference)
                .build();
            eprintln!(
                "LabSat observables diagnostic: source={:?}, timestamp-vs-nco raw={:.3} m, centered={:.3} m, tow-history spread={:.6}..{:.6} ms",
                observables.source,
                observables.reference_raw_delta_m,
                observables.reference_centered_delta_m,
                observable_inputs.tow_history_delta_min_ms,
                observable_inputs.tow_history_delta_max_ms
            );

            let solve_variant = |label: &str,
                                 epoch: &PseudorangeEpoch,
                                 mut config: ContinuousPvtConfig| {
                config.hatch = None;
                config.kalman = None;
                let mut pvt = ContinuousPvtRunner::new(config);
                let mut results = if nav.ephemeris_catalog.is_empty() {
                    pvt.solve_epochs(
                        std::slice::from_ref(epoch),
                        &nav.ephemerides,
                        &nav.ionosphere,
                        0.0,
                        0.0,
                        0.0,
                    )
                } else {
                    pvt.solve_epochs_with_catalog(
                        std::slice::from_ref(epoch),
                        &nav.ephemeris_catalog,
                        &nav.ionosphere,
                        0.0,
                        0.0,
                        0.0,
                    )
                };
                let result = results.pop().expect("LabSat diagnostic PVT result");
                let solution = &result.raw_solution;
                let diagnostic_count = solution.satellite_diagnostics.len().max(1) as f64;
                let iono_mean = solution
                    .satellite_diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.iono_delay_m)
                    .sum::<f64>()
                    / diagnostic_count;
                let tropo_mean = solution
                    .satellite_diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.tropo_delay_m)
                    .sum::<f64>()
                    / diagnostic_count;
                eprintln!(
                    "LabSat model diagnostic: {label}: converged={}, alt={:.3} m, residual_rms={:.3} m, iono_mean={iono_mean:.3} m, tropo_mean={tropo_mean:.3} m, sats={:?}",
                    solution.converged,
                    solution.lla[2],
                    solution.residual_rms_m,
                    solution.used_prns
                );
                solution.clone()
            };

            let baseline = solve_variant("iono=on,tropo=on", &epoch, base_config.clone());
            solve_variant(
                "observables=nco",
                &PseudorangeEpoch {
                    pseudoranges: observables.reference_pseudoranges,
                },
                base_config.clone(),
            );
            solve_variant(
                "observables=quantized-nco",
                &PseudorangeEpoch {
                    pseudoranges: from_common_receive_epoch(
                        &observable_inputs.quantized_reference,
                        common_rx_sample,
                        SAMPLE_RATE_HZ,
                        0.075,
                    ),
                },
                base_config.clone(),
            );
            solve_variant(
                "observables=nco+rx-rem",
                &PseudorangeEpoch {
                    pseudoranges: from_common_receive_epoch(
                        &observable_inputs.rx_rem_reference,
                        common_rx_sample,
                        SAMPLE_RATE_HZ,
                        0.075,
                    ),
                },
                base_config.clone(),
            );
            solve_variant(
                "observables=tow-history+code-phase",
                &PseudorangeEpoch {
                    pseudoranges: from_tow_history_common_receive_epoch(
                        &observable_inputs.tow_history_code_phase,
                        common_rx_sample,
                        SAMPLE_RATE_HZ,
                        0.075,
                    ),
                },
                base_config.clone(),
            );
            let mut no_iono = base_config.clone();
            no_iono.wls.apply_ionosphere = false;
            solve_variant("iono=off,tropo=on", &epoch, no_iono);
            let mut no_tropo = base_config.clone();
            no_tropo.wls.apply_troposphere = false;
            solve_variant("iono=on,tropo=off", &epoch, no_tropo);
            let mut no_atmosphere = base_config.clone();
            no_atmosphere.wls.apply_ionosphere = false;
            no_atmosphere.wls.apply_troposphere = false;
            solve_variant("iono=off,tropo=off", &epoch, no_atmosphere);

            for prn in baseline.used_prns {
                let mut reduced_epoch = epoch.clone();
                reduced_epoch
                    .pseudoranges
                    .retain(|pseudorange| pseudorange.prn != prn);
                solve_variant(
                    &format!("exclude=G{prn:02}"),
                    &reduced_epoch,
                    base_config.clone(),
                );
            }

            for diagnostic in baseline.satellite_diagnostics {
                let tgd_m = nav
                    .ephemerides
                    .get(&diagnostic.prn)
                    .map(|ephemeris| ephemeris.tgd * 299_792_458.0)
                    .unwrap_or(0.0);
                eprintln!(
                    "LabSat model diagnostic: G{:02}: residual={:.3} m, elevation={:.3} deg, iono={:.3} m, tropo={:.3} m, cTGD={tgd_m:.3} m",
                    diagnostic.prn,
                    diagnostic.residual_m,
                    diagnostic.elevation_rad.to_degrees(),
                    diagnostic.iono_delay_m,
                    diagnostic.tropo_delay_m
                );
            }
        } else {
            eprintln!(
                "LabSat model diagnostic: no latest pseudorange epoch; active tracking PRNs: {:?}",
                runner.active_tracking_prns()
            );
        }
    }

    eprintln!(
        "LabSat active tracking PRNs: {:?}",
        runner.active_tracking_prns()
    );
    if diagnostic_rinex_path.is_none() {
        assert!(
            runner.nav_state().rinex_nav_source.is_none(),
            "default LabSat PVT should use navigation-message ephemerides"
        );
        assert!(
            !runner.nav_state().ephemerides.is_empty(),
            "navigation messages should provide broadcast ephemerides"
        );
    }
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
    let expected_minute = if max_ms >= 120_000 { 56 } else { 55 };
    assert_eq!((utc.hour, utc.minute), (13, expected_minute));
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
