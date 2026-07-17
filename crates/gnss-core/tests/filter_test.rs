//! Test with fine doppler search around acq result.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
fn test_fine_doppler_search() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP"); return; }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let mut signal = vec![Complex::new(0.0, 0.0); spm * 2];
    reader.read(&mut signal, spm * 2).unwrap();
    let first_ms = &signal[..spm];

    // Coarse acquisition
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 500.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, first_ms);
    println!("Coarse: doppler={:.0} cp={:.1} pk={:.1}", ar.doppler_hz, ar.code_phase_chips, ar.peak_to_mean);

    // Fine search ±500 Hz around coarse, 50 Hz step
    let coarse_dopp = ar.doppler_hz;
    let fine_cfg = PcpsConfig { fs: FS, doppler_max: 600.0, doppler_step: 50.0, threshold: 0.0, ..Default::default() };
    let mut fine_acq = PcpsAcquisition::new(fine_cfg);
    
    // Manually search
    let mut best_dopp = coarse_dopp;
    let mut best_pk = 0.0;
    for dopp_offset in (-600..=600).step_by(50) {
        let dopp = coarse_dopp + dopp_offset as f64;
        // We need to create a temporary acq with this specific doppler
        // Actually, let's just try tracking with different initial dopplers
        let _ = dopp;
    }

    // Try tracking with different initial dopplers
    let total = (5.0 * FS) as usize;
    let mut buf = vec![Complex::new(0.0, 0.0); total];
    reader.seek_to_sample(0).unwrap();
    reader.read(&mut buf, total).unwrap();

    for dopp_offset in [0.0, -400.0, -450.0, -480.0, -500.0, -520.0, -550.0, -600.0] {
        let init_dopp = coarse_dopp + dopp_offset;
        let cfg = TrackingConfig { fs: FS, pll_bw: 40.0, fll_bw: 0.0, ..Default::default() };
        let mut trk = DllPllTracking::new(0, init_dopp, ar.code_phase_chips, cfg);
        
        // Track 2 seconds, check Q/I at 1.5s
        let mut best_qi = f64::MAX;
        for ms in 0..2000 {
            let out = trk.process(&buf[ms * spm..(ms + 1) * spm]);
            if ms == 1500 {
                let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
                best_qi = qi;
            }
        }
        println!("init_dopp={:.0}: Q/I at 1.5s = {:.3}", init_dopp, best_qi);
    }
}
