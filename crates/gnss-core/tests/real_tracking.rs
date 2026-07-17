//! Real data decoding with 3rd-order PLL + 2Msps resampling.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{decimate_by_2, IqFormat, IqReader};
use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS_IN: f64 = 4_000_000.0;
const FS: f64 = 2_000_000.0;

#[test]
fn test_real_data_tracking_and_decode() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP: no data"); return; }

    // Read 30s at 4Msps, decimate to 2Msps
    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let total_in = (30.0 * FS_IN) as usize;
    let mut buf_4m = vec![Complex::new(0.0, 0.0); total_in];
    let n = reader.read(&mut buf_4m, total_in).unwrap();
    let signal = decimate_by_2(&buf_4m[..n]);
    let spm = FS as usize / 1000;
    println!("Read {} samples at 2Msps ({:.1}s)", signal.len(), signal.len() as f64 / FS);

    // Acquire PRN 1
    // Acquire PRN 1 with finer doppler step
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 100.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);
    println!("Acq PRN 1: doppler={:.0}, code_phase={:.1}", ar.doppler_hz, ar.code_phase_chips);

    // Track with 3rd-order PLL
    let trk_cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);
    let mut dec = NavDecoder::new();
    let mut sf_count = 0u32;
    let mut sym_total = 0u32;
    let total_ms = signal.len() / spm;

    for ms in 0..total_ms {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        let out = trk.process(chunk);

        if let Some(sym) = trk.take_symbol() {
            sym_total += 1;
            if sym_total <= 12 || (sym_total >= 200 && sym_total <= 212) {
                println!("sym {}: I={:.0} dopp={:.1} cn0={:.1}",
                    sym_total-1, sym.re, out.carrier_doppler_hz, out.cn0_db_hz);
            }
            if dec.feed_symbol(sym.re) {
                sf_count += 1;
                if sf_count <= 5 {
                    println!("SF #{}: id={} TOW={} inverted={}",
                        sf_count, dec.subframe_id, dec.tow, dec.inverted);
                }
            }
        }
    }

    println!("Subframes decoded: {} ({} symbols)", sf_count, sym_total);
}
