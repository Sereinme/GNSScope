//! Real data decoding with corrected initial Doppler.

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
    if !data_path.exists() { eprintln!("SKIP"); return; }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (60.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    let n = reader.read(&mut signal, total).unwrap();
    let total_ms = n / spm;

    // Acquire with fine doppler step
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);
    println!("Acq: dopp={:.0} cp={:.1}", ar.doppler_hz, ar.code_phase_chips);

    // Fine doppler search: try offsets from acq result
    let mut best_qi = f64::MAX;
    let mut best_dopp = ar.doppler_hz;
    for offset in (-600..=0).step_by(40) {
        let init_dopp = ar.doppler_hz + offset as f64;
        let cfg = TrackingConfig { fs: FS, ..Default::default() };
        let mut trk = DllPllTracking::new(0, init_dopp, ar.code_phase_chips, cfg);
        for ms in 0..1500 {
            trk.process(&signal[ms * spm..(ms + 1) * spm]);
        }
        let out = trk.process(&signal[1500 * spm..1501 * spm]);
        let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
        if qi < best_qi { best_qi = qi; best_dopp = init_dopp; }
    }
    println!("Best init_dopp={:.0} Q/I={:.3}", best_dopp, best_qi);

    // Track 60s with best doppler
    let cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, best_dopp, ar.code_phase_chips, cfg);
    let mut prompt_i: Vec<f64> = Vec::with_capacity(total_ms);
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        prompt_i.push(out.prompt.re);
        if ms % 5000 == 0 {
            println!("ms {}: dopp={:.1} |I|={:.0}", ms, out.carrier_doppler_hz, out.prompt.re.abs());
        }
    }

    // 20-phase bit sync + NavDecoder
    let mut best_sf = 0u32;
    let mut best_boundary = 0;
    for boundary in 0..20 {
        let mut dec = NavDecoder::new();
        let mut sf_count = 0u32;
        let mut bit_idx = boundary;
        while bit_idx + 20 <= prompt_i.len() {
            let sum: f64 = prompt_i[bit_idx..bit_idx + 20].iter().sum();
            if dec.feed_symbol(sum) {
                sf_count += 1;
                if sf_count <= 3 {
                    println!("B{}: SF={} TOW={}", boundary, dec.subframe_id, dec.tow);
                }
            }
            bit_idx += 20;
        }
        if sf_count > best_sf { best_sf = sf_count; best_boundary = boundary; }
    }
    println!("Best: b={} sf={}", best_boundary, best_sf);
    assert!(best_sf >= 1, "Expected ≥1 subframe");
}
