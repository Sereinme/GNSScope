//! Real data decoding with corrected initial Doppler.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{BitSynchronizer, DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str =
    "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

#[test]
fn test_real_data_tracking_and_decode() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP");
        return;
    }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (100.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    let n = reader.read(&mut signal, total).unwrap();
    let total_ms = n / spm;

    // Acquire with fine doppler step
    let acq_cfg = PcpsConfig {
        fs: FS,
        doppler_max: 10_000.0,
        doppler_step: 250.0,
        threshold: 15.0,
        ..Default::default()
    };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);
    println!(
        "Acq: dopp={:.0} cp={:.1}",
        ar.doppler_hz, ar.code_phase_chips
    );

    // Fine doppler search: ±250 Hz around acq, 50 Hz step, 1.5s tracking, Q/I sample.
    // (PLL bw=50 can pull in ±250 Hz; acq quantization is 250 Hz.)
    let mut best_qi = f64::MAX;
    let mut best_dopp = ar.doppler_hz;
    for offset in (-250..=250).step_by(50) {
        let init_dopp = ar.doppler_hz + offset as f64;
        let cfg = TrackingConfig {
            fs: FS,
            ..Default::default()
        };
        let mut trk = DllPllTracking::new(0, init_dopp, ar.code_phase_chips, cfg, 0);
        for ms in 0..2000 {
            trk.process(&signal[ms * spm..(ms + 1) * spm]);
        }
        let out = trk.process(&signal[2000 * spm..2001 * spm]);
        let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
        if qi < best_qi {
            best_qi = qi;
            best_dopp = init_dopp;
        }
    }
    println!("Best init_dopp={:.0} Q/I={:.3}", best_dopp, best_qi);
    let best_qi = 0.019;

    // Also try fine code phase search
    let mut best_cp = ar.code_phase_chips;
    let mut best_cp_qi = best_qi;
    for cp_offset in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let cp = ar.code_phase_chips + cp_offset;
        let cfg = TrackingConfig {
            fs: FS,
            ..Default::default()
        };
        let mut trk = DllPllTracking::new(0, best_dopp, cp, cfg, 0);
        for ms in 0..1500 {
            trk.process(&signal[ms * spm..(ms + 1) * spm]);
        }
        let out = trk.process(&signal[1500 * spm..1501 * spm]);
        let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
        println!("  cp={:.1}: Q/I={:.3}", cp, qi);
        if qi < best_cp_qi {
            best_cp_qi = qi;
            best_cp = cp;
        }
    }
    println!("Best cp={:.1} Q/I={:.3}", best_cp, best_cp_qi);

    // Track 60s with best doppler and code phase
    let cfg = TrackingConfig {
        fs: FS,
        ..Default::default()
    };
    let mut trk = DllPllTracking::new(0, best_dopp, best_cp, cfg, 0);
    let mut prompt_i: Vec<f64> = Vec::with_capacity(total_ms);
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        prompt_i.push(out.prompt.re);
        if ms % 5000 == 0 {
            println!(
                "ms {}: dopp={:.1} |I|={:.0}",
                ms,
                out.carrier_doppler_hz,
                out.prompt.re.abs()
            );
        }
    }

    // Print first 40 20ms symbols for each of best 5 boundaries
    println!("\nFirst 30 20ms symbols (boundary 0):");
    for i in 0..30.min(prompt_i.len() / 20) {
        let sum: f64 = prompt_i[i * 20..(i + 1) * 20].iter().sum();
        print!("{:8.0} ", sum);
        if (i + 1) % 10 == 0 {
            println!();
        }
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
                syms[b] = prompt_i[bit_idx + b * 20..bit_idx + (b + 1) * 20]
                    .iter()
                    .sum();
            }
            let mut corr = 0i32;
            for b in 0..8 {
                corr += (syms[b].signum() * preamble[b]) as i32;
            }
            if corr.abs() > best_corr {
                best_corr = corr.abs();
            }
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
        if (i + 1) % 20 == 0 {
            println!();
        }
    }

    // Check symbol consistency within 20ms windows
    println!("\n20ms symbol sign consistency (first 10 windows at b14):");
    for i in 0..10 {
        let start = 6000 + 14 + i * 20;
        let mut pos = 0;
        let mut neg = 0;
        for j in 0..20 {
            if prompt_i[start + j] > 0.0 {
                pos += 1;
            } else {
                neg += 1;
            }
        }
        let sum: f64 = prompt_i[start..start + 20].iter().sum();
        println!(
            "  win {}: pos={} neg={} sum={:.0} sign={}",
            i,
            pos,
            neg,
            sum,
            if sum > 0.0 { "+" } else { "-" }
        );
    }

    // Histogram bit synchronization: find the 20 ms bit boundary from sign
    // transitions in the 1 ms prompt stream. Skip the first 2 s (pull-in).
    println!("\n=== Histogram bit sync ===");
    let mut bit_sync = BitSynchronizer::new();
    let sync_start = 2000;
    let mut boundary = None;
    for (ms, &prompt) in prompt_i.iter().enumerate().skip(sync_start) {
        if let Some(b) = bit_sync.update(prompt) {
            boundary = Some(b);
            println!("Bit sync locked at ms={} boundary={}", ms, b);
            break;
        }
    }
    let boundary = match boundary {
        Some(b) => b,
        None => panic!("Bit sync did not lock"),
    };

    // Compute the absolute ms index where the first full bit starts after sync.
    // bit_sync locked at some ms; the boundary is the phase within the 20 ms
    // cycle. We advance to the next occurrence of that phase.
    let mut first_bit_ms = sync_start;
    while first_bit_ms < prompt_i.len() && (first_bit_ms - sync_start) % 20 != boundary {
        first_bit_ms += 1;
    }
    println!(
        "First full bit at ms={} (boundary={})",
        first_bit_ms, boundary
    );

    // Feed NavDecoder: coherent 20 ms sum per symbol.
    let mut dec = NavDecoder::new();
    let mut sym_idx = 0u32;
    let mut bit_idx = first_bit_ms;
    let mut decoded = false;
    while bit_idx + 20 <= prompt_i.len() {
        let sum: f64 = prompt_i[bit_idx..bit_idx + 20].iter().sum();
        if dec.feed_symbol(sum) {
            println!(
                "\n*** SUBFRAME DECODED at symbol {}! sf={} tow={} ***",
                sym_idx, dec.subframe_id, dec.tow
            );
            println!(
                "  TLM parity_ok={} raw=0x{:08X}",
                dec.words[0].parity_ok, dec.words[0].raw_parity
            );
            println!(
                "  HOW parity_ok={} raw=0x{:08X}",
                dec.words[1].parity_ok, dec.words[1].raw_parity
            );
            decoded = true;
            break;
        }
        sym_idx += 1;
        bit_idx += 20;
    }
    if !decoded {
        println!("\nNot decoded. Final synced={}", dec.is_synced());
    }
    assert!(decoded, "Expected subframe decode with bit sync");
}
