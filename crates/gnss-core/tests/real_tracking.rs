//! Real data with 4→2Msps resampling (matching GNSS-SDR internal rate).
//! Tracks PRN 1, decodes nav message.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{decimate_by_2, IqFormat, IqReader};
use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS_IN: f64 = 4_000_000.0;   // file sample rate
const FS: f64 = 2_000_000.0;       // after decimation (matching GNSS-SDR)

#[test]
fn test_real_data_tracking_and_decode() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP: no data"); return; }

    // Read 30s at 4Msps, then decimate to 2Msps
    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm_in = FS_IN as usize / 1000; // 4000
    let spm = FS as usize / 1000;       // 2000

    let total_in = (30.0 * FS_IN) as usize;
    let mut buf_4m = vec![Complex::new(0.0, 0.0); total_in];
    let n = reader.read(&mut buf_4m, total_in).unwrap();
    let buf_2m = decimate_by_2(&buf_4m[..n]);

    // Acquisition at 2Msps on first 2ms
    let first_1ms = &buf_2m[..spm];
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 500.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, first_1ms);
    assert!(ar.detected);
    println!("Acq PRN 1: doppler={:.0}, code_phase={:.1}", ar.doppler_hz, ar.code_phase_chips);

    // Track at 2Msps
    let trk_cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);

    let total_ms = buf_2m.len() / spm;
    let mut prompt_buf: Vec<f64> = Vec::with_capacity(total_ms);

    for ms in 0..total_ms {
        let chunk = &buf_2m[ms * spm..(ms + 1) * spm];
        let out = trk.process(chunk);
        prompt_buf.push(out.prompt.re);
    }

    // 20 parallel decoders, 20 possible 20ms boundaries
    let mut decoders: [NavDecoder; 20] = std::array::from_fn(|_| NavDecoder::new());
    let mut sf_count = 0u32;
    let mut bit_count: [u32; 20] = [0; 20];

    for boundary in 0..20 {
        let mut bit_idx = boundary;
        while bit_idx + 20 <= prompt_buf.len() {
            let sum: f64 = prompt_buf[bit_idx..bit_idx + 20].iter().sum();
            let bit = if sum > 0.0 { 1u8 } else { 0u8 };
            if decoders[boundary].feed_bit(bit) {
                sf_count += 1;
                if sf_count <= 3 {
                    println!("Phase {}: SF={} TOW={}",
                        boundary, decoders[boundary].subframe_id, decoders[boundary].tow);
                }
            }
            bit_count[boundary] += 1;
            bit_idx += 20;
        }
    }

    println!("Bits per phase: {:?}", &bit_count[..5]);
    if sf_count == 0 {
        println!("Phase 0 first 50 bits:");
        let mut s = String::new();
        for i in 0..50.min(prompt_buf.len() / 20) {
            let sum: f64 = prompt_buf[i*20..(i+1)*20].iter().sum();
            s.push(if sum > 0.0 { '1' } else { '0' });
        }
        println!("  {}", s);
    }
    println!("Subframes decoded: {}", sf_count);
}
