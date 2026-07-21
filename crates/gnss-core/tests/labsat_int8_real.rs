use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::receiver::{StreamingPvtConfig, StreamingPvtEvent, StreamingPvtRunner};
use gnss_data::gps_l1ca::PRN_COUNT;
use num_complex::Complex;
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
            result.prn + 1,
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
    for expected_prn in [11, 13, 24, 29] {
        assert!(
            detections.iter().any(|result| result.prn == expected_prn),
            "expected stable LabSat detection G{expected_prn:02}"
        );
    }
}

#[test]
fn labsat_int8_iq_enters_streaming_tracking() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: LabSat Int8 real IQ data not present");
        return;
    }

    let mut config = StreamingPvtConfig::cttc_fixed_prns(vec![11]);
    config.iq_path = Some(data_path.to_path_buf());
    config.iq_format = IqFormat::Int8;
    config.sample_rate_hz = SAMPLE_RATE_HZ;
    config.intermediate_frequency_hz = 0.0;
    config.chunk_ms = 10;
    config.acquisition.acq_window_ms = 10;
    config.acquisition.noncoherent_integrations = 10;
    config.acquisition.cold_acq_interval_ms = 1;
    config.acquisition.doppler_step_hz = 500.0;
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
                            .any(|result| result.prn == 11 && result.detected);
                    }
                    StreamingPvtEvent::TrackingBatch(updates) => {
                        tracking_updated |= updates.iter().any(|update| update.prn == 11);
                    }
                    _ => {}
                }
            }
        })
        .expect("stream LabSat Int8 IQ");

    assert!(acquisition_detected, "streaming runner did not acquire G11");
    assert!(tracking_updated, "streaming runner did not track G11");
    assert_eq!(runner.active_tracking_prns(), vec![11]);
}
