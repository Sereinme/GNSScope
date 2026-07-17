//! Real data decoding with GNSS-SDR compatible PLL.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
fn test_real_data_tracking_and_decode() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP: no data"); return; }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (60.0 * FS) as usize;  // 60 seconds
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    let n = reader.read(&mut signal, total).unwrap();

    // Acquire PRN 1
    let first_1ms = &signal[..spm];
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, first_1ms);
    assert!(ar.detected);
    println!("Acq PRN 1: doppler={:.0}, code_phase={:.1}", ar.doppler_hz, ar.code_phase_chips);

    let trk_cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);
    let mut dec = NavDecoder::new();
    let mut sf_count = 0u32;
    let total_ms = n / spm;

    for ms in 0..total_ms {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        let _out = trk.process(chunk);

        if let Some(sym) = trk.take_symbol() {
            if dec.feed_symbol(sym.re) {
                sf_count += 1;
                if sf_count <= 5 {
                    println!("SF #{}: id={} TOW={} inverted={}",
                        sf_count, dec.subframe_id, dec.tow, dec.inverted);
                }
            }
        }
    }

    println!("Subframes decoded: {}", sf_count);
    assert!(sf_count >= 1, "Expected ≥1 subframe from 60s real GPS data, got {}", sf_count);
}
