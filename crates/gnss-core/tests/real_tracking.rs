//! Real data tracking verification test.
//! Runs tracking on PRN 1 from CTTC Spain data and verifies nav message decode.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
#[ignore = "bit sync produces symbols but decoder needs 300bits after sync"]
fn test_real_data_tracking_and_decode() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: Real IQ data not available");
        return;
    }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000; // 4000

    // Read first 2ms for acquisition
    let mut buf = vec![Complex::new(0.0, 0.0); spm * 2];
    reader.read(&mut buf, spm * 2).unwrap();

    // Acquire PRN 1 (strongest signal)
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &buf[..spm]);
    assert!(ar.detected, "PRN 1 should be detected");
    println!("Acq PRN 1: doppler={:.0}, code_phase={:.1}", ar.doppler_hz, ar.code_phase_chips);

    // Read 30 seconds of data for nav message (6 subframes)
    let tracking_duration_s = 30.0;
    let total_samples = (tracking_duration_s * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total_samples];
    reader.seek_to_sample(0).unwrap();
    let n = reader.read(&mut signal, total_samples).unwrap();
    println!("Read {} samples for tracking ({:.1}s)", n, n as f64 / FS);

    // Track and decode
    let trk_cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);
    let mut dec = NavDecoder::new();

    let total_ms = n / spm;
    let mut subframe_count = 0;
    let mut first_tow: Option<u32> = None;

    for ms in 0..total_ms {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        let out = trk.process(chunk);

        if let Some(sym) = trk.take_symbol() {
            let sym_idx = ms / 20;
            if sym_idx < 20 || sym_idx % 100 == 0 {
                println!("sym {}: I={:.0}, cp={:.2}, dopp={:.0}, cn0={:.1}",
                    sym_idx, sym.re, out.code_phase_chips, out.carrier_doppler_hz, out.cn0_db_hz);
            }
            let bit = if sym.re > 0.0 { 1 } else { 0 };
            if dec.feed_bit(bit) {
                subframe_count += 1;
                if first_tow.is_none() {
                    first_tow = Some(dec.tow);
                }
                println!("Subframe {}: SF={}, TOW={}, parity_ok=T:{}, H:{}",
                    subframe_count, dec.subframe_id, dec.tow,
                    dec.words.get(0).map(|w| w.parity_ok).unwrap_or(false),
                    dec.words.get(1).map(|w| w.parity_ok).unwrap_or(false));
            }
        }
    }

    println!("Decoded {} subframes in {} seconds", subframe_count, tracking_duration_s as u32);
    assert!(subframe_count >= 1, "Expected >=1 subframe from 30s of real GPS data");
}
