//! Real data bit-sync + decode test.
//! Tracks PRN 1, outputs 1ms prompt values, decoder tries all 20 20ms boundaries.

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
    let mut buf = vec![Complex::new(0.0, 0.0); spm * 2];
    reader.read(&mut buf, spm * 2).unwrap();

    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &buf[..spm]);
    assert!(ar.detected);
    println!("Acq PRN 1: doppler={:.0}, code_phase={:.1}", ar.doppler_hz, ar.code_phase_chips);

    // Read 30s of data
    let dur_s = 100.0;
    let total = (dur_s * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.seek_to_sample(0).unwrap();
    let n = reader.read(&mut signal, total).unwrap();
    println!("Read {} samples ({:.0}s)", n, n as f64 / FS);

    let trk_cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);

    // Buffer 1ms prompt I values
    let total_ms = n / spm;
    let mut prompt_buf: Vec<f64> = Vec::with_capacity(total_ms);

    // 20 parallel decoders, each at a different 20ms boundary
    let mut decoders: [NavDecoder; 20] = std::array::from_fn(|_| NavDecoder::new());
    let mut sf_count = 0u32;
    let mut bit_count: [u32; 20] = [0; 20];

    for ms in 0..total_ms {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        let out = trk.process(chunk);
        prompt_buf.push(out.prompt.re);
    }

    // Try all 20 possible 20ms boundaries
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

    println!("Bits per phase: {:?}", bit_count.iter().map(|c| c).take(5).collect::<Vec<_>>());
    // Print first 50 bits of phase 0 for manual preamble check
    if sf_count == 0 {
        println!("Phase 0 first 50 bits:");
        let mut s = String::new();
        for i in 0..50 {
            if i < prompt_buf.len() / 20 {
                let sum: f64 = prompt_buf[i*20..(i+1)*20].iter().sum();
                s.push(if sum > 0.0 { '1' } else { '0' });
            }
        }
        println!("  {}", s);
    }
    println!("Subframes decoded: {}", sf_count);
}
