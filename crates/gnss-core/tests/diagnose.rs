//! Quick diagnostic: track PRN 1 for 500ms, show freq + prompt.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
fn diagnose_freq_tracking() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP"); return; }
    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (1.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.read(&mut signal, total).unwrap();

    // Acquire PRN 1
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);

    let trk_cfg = TrackingConfig { fs: FS, coherent_ms: 1, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);

    let total_ms = total / spm;
    println!("Acq: doppler={:.0}, tracking {}ms...", ar.doppler_hz, total_ms);
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        if ms % 50 == 0 {
            println!("ms {:4}: freq={:8.1} |I|={:6.0} |Q|={:6.0} Q/I={:6.3} cp={:.2}",
                ms, out.carrier_doppler_hz,
                out.prompt.re.abs(), out.prompt.im.abs(),
                (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs(),
                out.code_phase_chips);
        }
    }
}
