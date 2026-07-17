//! Quick PLL/FLL discriminator diagnostic.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
fn diagnose_pll_fll() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP"); return; }
    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (5.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.read(&mut signal, total).unwrap();

    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);

    let trk_cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);
    let total_ms = total / spm;

    println!("ms    pll_in(cyc) fll_in(Hz)  Doppler  |I|      |Q|");
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        if ms % 200 == 0 || ms < 10 {
            println!("{:4}  {:10.4} {:10.1} {:8.1} {:8.0} {:8.0}",
                ms, trk.last_pll_in(), trk.last_fll_in(),
                out.carrier_doppler_hz,
                out.prompt.re.abs(), out.prompt.im.abs());
        }
    }
}
