//! Isolated release benchmark for the public tracking correlator.

use gnss_core::io::{IqFormat, IqReader};
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

const DATA_FILE: &str =
    "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
#[ignore = "isolated tracking throughput benchmark; use a release build"]
fn benchmark_tracking_correlator_on_real_iq() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real data not present");
        return;
    }

    let iterations = std::env::var("GNSSCOPE_TRACKING_BENCHMARK_MS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100_000);
    let samples_per_ms = FS as usize / 1000;
    let mut samples = vec![Complex::new(0.0, 0.0); samples_per_ms];
    let mut reader = IqReader::open(data_path, IqFormat::Int16).expect("open CTTC IQ");
    let samples_read = reader
        .read(&mut samples, samples_per_ms)
        .expect("read one CTTC millisecond");
    assert_eq!(samples_read, samples_per_ms);

    let config = TrackingConfig {
        fs: FS,
        ..Default::default()
    };
    let mut tracker = DllPllTracking::new(0, 7_250.0, 0.0, config, 0);
    for _ in 0..1_000 {
        black_box(tracker.process(black_box(&samples)));
    }

    let started = Instant::now();
    for _ in 0..iterations {
        black_box(tracker.process(black_box(&samples)));
    }
    let elapsed = started.elapsed();
    eprintln!(
        "tracked {iterations} ms in {:.6} s ({:.3} us/ms)",
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1e6 / iterations as f64,
    );
}
