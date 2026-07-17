//! Deep diagnostic: DLL discriminator + code freq for PRN 1.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
fn diagnose_dll() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP"); return; }
    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (2.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.read(&mut signal, total).unwrap();

    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);

    let trk_cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);

    let total_ms = total / spm;
    println!("ms    |I|       |Q|       Q/I     cp       code_freq      dll_err    dll_state");
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        if ms % 50 == 0 {
            // We can't directly get dll_err from TrackingOutput, but we can get code_freq and dll_state
            println!("{:4} {:8.0} {:8.0} {:7.3} {:7.2} {:12.1} {:10.2}",
                ms,
                out.prompt.re.abs(), out.prompt.im.abs(),
                (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs(),
                out.code_phase_chips,
                trk.code_freq(),
                trk.dll_state());
        }
    }
}
