//! Release benchmark for the production-shaped batched PCPS acquisition path.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use num_complex::Complex;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

const DATA_FILE: &str = "../../data/202306202155/Data202306202155(2).dat";
const FS: f64 = 10_230_000.0;
const PRNS: &[usize] = &[9, 11, 22, 27];

#[test]
#[ignore = "batched acquisition throughput benchmark; use a release build"]
fn benchmark_batched_pcps_on_real_labsat_iq() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real LabSat data not present");
        return;
    }

    let iterations = std::env::var("GNSSCOPE_ACQUISITION_BENCHMARK_RUNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5);
    let integrations = 10;
    let sample_count = FS as usize / 1_000 * integrations;
    let mut samples = vec![Complex::new(0.0, 0.0); sample_count];
    let mut reader = IqReader::open(data_path, IqFormat::Int8).expect("open LabSat IQ");
    let samples_read = reader
        .read(&mut samples, sample_count)
        .expect("read LabSat acquisition window");
    assert_eq!(samples_read, sample_count);

    let config = PcpsConfig {
        fs: FS,
        if_freq: 0.0,
        doppler_center_hz: 0.0,
        doppler_max: 10_000.0,
        doppler_step: 250.0,
        coherent_integration_ms: 1.0,
        noncoherent_integrations: integrations,
        threshold: 3.0,
    };
    let mut acquisition = PcpsAcquisition::new(config);
    black_box(acquisition.acquire_many_at_sample(PRNS, black_box(&samples), 0));

    let started = Instant::now();
    for _ in 0..iterations {
        black_box(acquisition.acquire_many_at_sample(PRNS, black_box(&samples), 0));
    }
    let elapsed = started.elapsed();
    eprintln!(
        "completed {iterations} batched LabSat acquisitions in {:.6} s ({:.3} ms/scan)",
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1e3 / iterations as f64,
    );
}
