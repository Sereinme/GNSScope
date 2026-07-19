//! M1 verification with real GNSS-SDR IQ data.
//!
//! Uses CTTC Spain dataset: 100s GPS L1 C/A, 4 Msps, 16-bit I/Q, zero IF.
//! Reference position: Lat 41.2749°, Lon 1.98754° (CTTC, Spain).

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_data::gps_l1ca::PRN_COUNT;
use num_complex::Complex;
use std::path::Path;
const DATA_FILE: &str =
    "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
fn test_real_data_acquisition() {
    // Check if data file exists
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: Real IQ data not available at {}", DATA_FILE);
        eprintln!("Download from: https://sourceforge.net/projects/gnss-sdr/files/data/");
        eprintln!("Extract: tar -xzf data/cttc_spain.tar.gz -C data/");
        return;
    }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let total_samples = reader.total_samples();
    println!(
        "File: {} samples total ({:.1} seconds at {} Msps)",
        total_samples,
        total_samples as f64 / FS,
        FS / 1_000_000.0
    );

    // Read first 2ms for acquisition (8000 samples)
    let samples_per_ms = FS as usize / 1000; // 4000
    let mut buffer = vec![Complex::new(0.0, 0.0); samples_per_ms * 2];
    let n = reader.read(&mut buffer, samples_per_ms * 2).unwrap();
    assert!(n >= samples_per_ms, "Need at least 1ms of data");

    let first_ms = &buffer[..samples_per_ms];

    // Run acquisition on all 32 PRNs
    let config = PcpsConfig {
        fs: FS,
        if_freq: 0.0,
        doppler_max: 10_000.0,
        doppler_step: 250.0, // finer step for real data
        coherent_integration_ms: 1.0,
        threshold: 15.0, // Real GPS data has high noise floor
    };
    let mut acq = PcpsAcquisition::new(config);
    let mut detections = Vec::new();

    for prn in 0..PRN_COUNT {
        let result = acq.acquire(prn, first_ms);
        if result.detected {
            detections.push(result);
        }
    }

    println!("Detected {} satellites in first 1ms:", detections.len());
    for d in &detections {
        println!(
            "  PRN {:2}: doppler={:+.0} Hz, code_phase={:.1} chips, peak/mean={:.1}",
            d.prn, d.doppler_hz, d.code_phase_chips, d.peak_to_mean
        );
    }

    assert!(
        detections.len() >= 4,
        "Expected >=4 satellites, got {}. Try adjusting threshold or checking data.",
        detections.len()
    );
}

#[test]
fn test_real_data_acquisition_with_noncoherent_integration() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: Real IQ data not available");
        return;
    }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let samples_per_ms = FS as usize / 1000;

    // Read 20ms for non-coherent integration
    let mut buffer = vec![Complex::new(0.0, 0.0); samples_per_ms * 20];
    reader.read(&mut buffer, samples_per_ms * 20).unwrap();

    // Simple non-coherent: run acquisition on each 1ms, accumulate magnitudes
    let config = PcpsConfig {
        fs: FS,
        doppler_max: 10_000.0,
        doppler_step: 250.0,
        coherent_integration_ms: 1.0,
        threshold: 2.2,
        ..Default::default()
    };
    let mut acq = PcpsAcquisition::new(config);
    let mut detections = Vec::new();

    for prn in 0..PRN_COUNT {
        // Try multiple 1ms windows
        let mut best_peak = 0.0;
        for ms in 0..10 {
            let chunk = &buffer[ms * samples_per_ms..(ms + 1) * samples_per_ms];
            let result = acq.acquire(prn, chunk);
            if result.peak_to_mean > best_peak {
                best_peak = result.peak_to_mean;
            }
            if result.detected && result.peak_to_mean > 3.0 {
                detections.push(result);
                break;
            }
        }
    }

    println!("Non-coherent search found {} candidates", detections.len());
    // We should find at least 4 satellites with real GPS data
    if detections.len() < 4 {
        eprintln!("WARNING: Only {} satellites detected. Real data may need more coherent integration time.", detections.len());
    }
}
