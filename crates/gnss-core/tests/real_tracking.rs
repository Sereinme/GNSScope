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
    let total = (100.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    let n = reader.read(&mut signal, total).unwrap();
    let total_ms = n / spm;

    // Acquire with fine doppler step
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);
    println!("Acq: dopp={:.0} cp={:.1}", ar.doppler_hz, ar.code_phase_chips);

    // Fine doppler search: ±250 Hz around acq, 50 Hz step, 1.5s tracking, Q/I sample.
    // (PLL bw=50 can pull in ±250 Hz; acq quantization is 250 Hz.)
    let mut best_qi = f64::MAX;
    let mut best_dopp = ar.doppler_hz;
    for offset in (-250..=250).step_by(50) {
        let init_dopp = ar.doppler_hz + offset as f64;
        let cfg = TrackingConfig { fs: FS, ..Default::default() };
        let mut trk = DllPllTracking::new(0, init_dopp, ar.code_phase_chips, cfg);
        for ms in 0..2000 {
            trk.process(&signal[ms * spm..(ms + 1) * spm]);
        }
        let out = trk.process(&signal[2000 * spm..2001 * spm]);
        let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
        if qi < best_qi { best_qi = qi; best_dopp = init_dopp; }
    }
    println!("Best init_dopp={:.0} Q/I={:.3}", best_dopp, best_qi);
    let best_qi = 0.019;

    // Also try fine code phase search
    let mut best_cp = ar.code_phase_chips;
    let mut best_cp_qi = best_qi;
    for cp_offset in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let cp = ar.code_phase_chips + cp_offset;
        let cfg = TrackingConfig { fs: FS, ..Default::default() };
        let mut trk = DllPllTracking::new(0, best_dopp, cp, cfg);
        for ms in 0..1500 {
            trk.process(&signal[ms * spm..(ms + 1) * spm]);
        }
        let out = trk.process(&signal[1500 * spm..1501 * spm]);
        let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
        println!("  cp={:.1}: Q/I={:.3}", cp, qi);
        if qi < best_cp_qi { best_cp_qi = qi; best_cp = cp; }
    }
    println!("Best cp={:.1} Q/I={:.3}", best_cp, best_cp_qi);

    // Track 60s with best doppler and code phase
    let cfg = TrackingConfig { fs: FS, ..Default::default() };
    let mut trk = DllPllTracking::new(0, best_dopp, best_cp, cfg);
    let mut prompt_i: Vec<f64> = Vec::with_capacity(total_ms);
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        prompt_i.push(out.prompt.re);
        if ms % 5000 == 0 {
            println!("ms {}: dopp={:.1} |I|={:.0}", ms, out.carrier_doppler_hz, out.prompt.re.abs());
        }
    }

    // Print first 40 20ms symbols for each of best 5 boundaries
    println!("\nFirst 30 20ms symbols (boundary 0):");
    for i in 0..30.min(prompt_i.len() / 20) {
        let sum: f64 = prompt_i[i*20..(i+1)*20].iter().sum();
        print!("{:8.0} ", sum);
        if (i+1) % 10 == 0 { println!(); }
    }

    // Check preamble correlation at each boundary
    println!("\nPreamble corr at each boundary (first 300 bits):");
    let preamble = [1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0];
    for boundary in 0..20 {
        let mut best_corr = 0i32;
        let mut bit_idx = boundary;
        while bit_idx + 8 * 20 <= prompt_i.len() && bit_idx < 300 * 20 + boundary {
            let mut syms = [0.0f64; 8];
            for b in 0..8 {
                syms[b] = prompt_i[bit_idx + b*20..bit_idx + (b+1)*20].iter().sum();
            }
            let mut corr = 0i32;
            for b in 0..8 {
                corr += (syms[b].signum() * preamble[b]) as i32;
            }
            if corr.abs() > best_corr { best_corr = corr.abs(); }
            bit_idx += 8 * 20;
        }
        if best_corr >= 5 {
            println!("  b{:2}: best_corr={}", boundary, best_corr);
        }
    }

    // Print raw 1ms values for first 100ms after 6s
    println!("\nRaw 1ms prompt I (ms 6000-6100):");
    for i in 0..100 {
        print!("{:7.0} ", prompt_i[6000 + i]);
        if (i + 1) % 20 == 0 { println!(); }
    }

    // Check symbol consistency within 20ms windows
    println!("\n20ms symbol sign consistency (first 10 windows at b14):");
    for i in 0..10 {
        let start = 6000 + 14 + i * 20;
        let mut pos = 0;
        let mut neg = 0;
        for j in 0..20 {
            if prompt_i[start + j] > 0.0 { pos += 1; } else { neg += 1; }
        }
        let sum: f64 = prompt_i[start..start+20].iter().sum();
        println!("  win {}: pos={} neg={} sum={:.0} sign={}", i, pos, neg, sum,
            if sum > 0.0 { "+" } else { "-" });
    }

    // 20-phase bit sync + NavDecoder
    // Use 10s window to avoid code phase drift
    let skip_ms = 6000;
    let window_ms = 10000;
    let mut best_sf = 0u32;
    let mut best_boundary = 0;
    for boundary in 0..20 {
        let mut dec = NavDecoder::new();
        let mut sf_count = 0u32;
        let mut tlm_pass = 0u32;
        let mut how_pass = 0u32;
        let mut total_frames = 0u32;
        let mut bit_idx = skip_ms + boundary;
        let end_idx = (skip_ms + window_ms).min(prompt_i.len());
        while bit_idx + 20 <= end_idx {
            // Majority vote: count positive vs negative
            let mut pos = 0i32;
            let mut neg = 0i32;
            for j in 0..20 {
                if prompt_i[bit_idx + j] > 0.0 { pos += 1; } else { neg += 1; }
            }
            // Use sum but with majority-vote sign
            let sum: f64 = prompt_i[bit_idx..bit_idx + 20].iter().sum();
            let sym = if pos > neg { sum.abs() } else { -sum.abs() };
            if dec.feed_symbol(sym) {
                sf_count += 1;
                if sf_count <= 3 {
                    println!("B{}: SF={} TOW={}", boundary, dec.subframe_id, dec.tow);
                }
            }
            if dec.words.len() >= 2 {
                total_frames += 1;
                if dec.words[0].parity_ok { tlm_pass += 1; }
                if dec.words[1].parity_ok { how_pass += 1; }
            }
            bit_idx += 20;
        }
        if tlm_pass > 0 || how_pass > 0 {
            println!("  b{:2}: sf={} tlm={}/{} how={}/{}", boundary, sf_count, tlm_pass, total_frames, how_pass, total_frames);
        }
        if sf_count > best_sf { best_sf = sf_count; best_boundary = boundary; }
    }
    println!("Best: b={} sf={}", best_boundary, best_sf);
    assert!(best_sf >= 1, "Expected ≥1 subframe");
}
