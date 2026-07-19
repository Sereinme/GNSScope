//! Bit sync / frame sync analysis on synthetic signal (PLL locked).
//! Isolates NavDecoder from real-data noise.

use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

const FS: f64 = 4_000_000.0;
const PREAMBLE_BITS: [u8; 8] = [1, 0, 0, 0, 1, 0, 1, 1];

/// Build a synthetic GPS L1 C/A signal with a known nav bit stream.
/// bits: nav bits (0/1), each bit lasts 20ms.
fn build_signal(prn: usize, doppler: f64, bits: &[u8], amp: f64) -> Vec<Complex<f64>> {
    let spm = FS as usize / 1000;
    let spc = FS / 1.023e6;
    let code = &PRN_CODES[prn];
    let dur_ms = bits.len() * 20;
    (0..dur_ms * spm)
        .map(|i| {
            let n = i % spm;
            let ms = i / spm;
            let bi = ms / 20;
            let bm = if bits[bi] == 1 { 1.0 } else { -1.0 };
            let ci = (n as f64 / spc) as usize % CODE_LENGTH;
            let phase = TAU * doppler * i as f64 / FS;
            Complex::new(phase.cos(), phase.sin()) * Complex::new(amp * code[ci] as f64 * bm, 0.0)
        })
        .collect()
}

/// Generate a valid GPS subframe word. Parity per IS-GPS-200 (verified
/// against GNSS-SDR gps_word_parityCheck masks).
/// D29*/D30* are the last 2 bits of the previous word (D29*, D30*).
fn make_word(data24: u32, d29_star: u8, d30_star: u8) -> u32 {
    let w = (data24 & 0xFFFFFF) << 6; // data in D1-D24 = bits 29..6
    let d = |n: u32| -> u8 { ((w >> (30 - n)) & 1) as u8 };
    let p25 = d29_star
        ^ d(1)
        ^ d(2)
        ^ d(3)
        ^ d(5)
        ^ d(6)
        ^ d(10)
        ^ d(11)
        ^ d(12)
        ^ d(13)
        ^ d(14)
        ^ d(17)
        ^ d(18)
        ^ d(20)
        ^ d(23);
    let p26 = d30_star
        ^ d(2)
        ^ d(3)
        ^ d(4)
        ^ d(6)
        ^ d(7)
        ^ d(11)
        ^ d(12)
        ^ d(13)
        ^ d(14)
        ^ d(15)
        ^ d(18)
        ^ d(19)
        ^ d(21)
        ^ d(24);
    let p27 = d29_star
        ^ d(1)
        ^ d(3)
        ^ d(4)
        ^ d(5)
        ^ d(7)
        ^ d(8)
        ^ d(12)
        ^ d(13)
        ^ d(14)
        ^ d(15)
        ^ d(16)
        ^ d(19)
        ^ d(20)
        ^ d(22);
    let p28 = d30_star
        ^ d(2)
        ^ d(4)
        ^ d(5)
        ^ d(6)
        ^ d(8)
        ^ d(9)
        ^ d(13)
        ^ d(14)
        ^ d(15)
        ^ d(16)
        ^ d(17)
        ^ d(20)
        ^ d(21)
        ^ d(23);
    let p29 = d30_star
        ^ d(1)
        ^ d(5)
        ^ d(6)
        ^ d(7)
        ^ d(9)
        ^ d(10)
        ^ d(14)
        ^ d(15)
        ^ d(16)
        ^ d(17)
        ^ d(18)
        ^ d(21)
        ^ d(22)
        ^ d(24);
    let p30 = d29_star
        ^ d(3)
        ^ d(4)
        ^ d(5)
        ^ d(6)
        ^ d(8)
        ^ d(9)
        ^ d(10)
        ^ d(11)
        ^ d(13)
        ^ d(15)
        ^ d(19)
        ^ d(22)
        ^ d(23)
        ^ d(24);
    w | (p25 as u32) << 5
        | (p26 as u32) << 4
        | (p27 as u32) << 3
        | (p28 as u32) << 2
        | (p29 as u32) << 1
        | p30 as u32
}

fn word_to_bits(w: u32) -> Vec<u8> {
    (0..30).rev().map(|i| ((w >> i) & 1) as u8).collect()
}

#[test]
fn test_synthetic_subframe_decode() {
    let prn = 5;
    let doppler = 1500.0_f64; // integer cycles/ms for clean lock
    let amp = 3.0;

    // Build subframe bits: TLM + HOW + 8 zero words
    // TLM: preamble (0x8B = 10001011) in bits 1-8, rest 0
    let tlm_data24: u32 = 0x8B << 16; // preamble in top 8 bits
    let tlm = make_word(tlm_data24, 0, 0);
    let td29 = ((tlm >> 1) & 1) as u8;
    let td30 = (tlm & 1) as u8;
    // HOW: TOW=100, subframe_id=1
    let how_data24: u32 = (100 << 13) | (1 << 8);
    let how = make_word(how_data24, td29, td30);
    println!("TLM = 0x{:08X}, HOW = 0x{:08X}", tlm, how);
    // Verify TLM parity standalone (D29*=0, D30*=0 → no inversion)
    // ext_word for TLM: d30*=0 so no inversion, just gpsword
    let tlm_ext = tlm; // d30*=0
    println!(
        "TLM standalone parity (gps_parity_check on 0x{:08X}): would need access",
        tlm_ext
    );
    let mut bits: Vec<u8> = Vec::new();
    bits.extend(word_to_bits(tlm));
    bits.extend(word_to_bits(how));
    for _ in 0..8 {
        let prev_d29 = (how & 1) as u8; // simplified: use last word's D29/D30
        let prev_d30 = (how & 1) as u8;
        bits.extend(word_to_bits(make_word(0, prev_d29, prev_d30)));
    }
    println!("TLM = 0x{:08X}, HOW = 0x{:08X}", tlm, how);
    println!("First 30 bits: {:?}", &bits[..30.min(bits.len())]);

    // Repeat subframe 3x for enough data after bit sync
    let bits3: Vec<u8> = bits.iter().cycle().take(bits.len() * 3).copied().collect();
    let signal = build_signal(prn, doppler, &bits3, amp);
    let spm = FS as usize / 1000;

    // Track with exact initial conditions (PLL locks immediately)
    let cfg = TrackingConfig {
        fs: FS,
        pll_bw: 50.0,
        pll_order: 3,
        ..Default::default()
    };
    let mut trk = DllPllTracking::new(prn, doppler, 0.0, cfg, 0);

    // Collect prompt I every ms
    let total_ms = signal.len() / spm;
    let mut prompt_i: Vec<f64> = Vec::with_capacity(total_ms);
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        prompt_i.push(out.prompt.re);
    }

    // Print first 30 20ms-summed symbols (boundary 0)
    println!("\nFirst 30 20ms symbols (boundary 0):");
    for i in 0..30 {
        let sum: f64 = prompt_i[i * 20..(i + 1) * 20].iter().sum();
        let sign = if sum > 0.0 { 1 } else { 0 };
        print!("{}({:.0}) ", sign, sum);
        if (i + 1) % 10 == 0 {
            println!();
        }
    }

    // Expected: first 8 symbols = preamble [1,-1,-1,-1,1,-1,1,1]
    println!("\nExpected preamble symbols: {:?}", PREAMBLE_BITS);

    // Try NavDecoder at boundary 0
    let mut dec = NavDecoder::new();
    let mut decoded = false;
    for i in 0..prompt_i.len() / 20 {
        let sum: f64 = prompt_i[i * 20..(i + 1) * 20].iter().sum();
        if dec.feed_symbol(sum) {
            println!(
                "\n*** SUBFRAME DECODED! sf={} tow={} ***",
                dec.subframe_id, dec.tow
            );
            println!("  TLM parity_ok={}", dec.words[0].parity_ok);
            println!("  HOW parity_ok={}", dec.words[1].parity_ok);
            decoded = true;
            break;
        }
    }
    if !decoded {
        println!("\nNavDecoder did not decode. Checking preamble detection...");
        // Feed symbols and watch synced state
        let mut dec2 = NavDecoder::new();
        for i in 0..50.min(prompt_i.len() / 20) {
            let sum: f64 = prompt_i[i * 20..(i + 1) * 20].iter().sum();
            dec2.feed_symbol(sum);
            println!("sym {:2}: sum={:10.0} synced={}", i, sum, dec2.is_synced());
        }
        // Also test NavDecoder with PERFECT symbols (±1) to isolate parity logic
        // from tracking symbol magnitude/phase issues.
        println!("\n=== NavDecoder with perfect ±1 symbols ===");
        let perfect_syms: Vec<f64> = bits3
            .iter()
            .map(|&b| if b == 1 { 1.0 } else { -1.0 })
            .collect();
        let mut dec3 = NavDecoder::new();
        let mut decoded3 = false;
        for (i, &s) in perfect_syms.iter().enumerate() {
            if dec3.feed_symbol(s) {
                println!(
                    "Perfect symbols: DECODED at bit {}! sf={} tow={}",
                    i, dec3.subframe_id, dec3.tow
                );
                println!(
                    "  TLM parity_ok={} raw=0x{:08X}",
                    dec3.words[0].parity_ok, dec3.words[0].raw_parity
                );
                println!(
                    "  HOW parity_ok={} raw=0x{:08X}",
                    dec3.words[1].parity_ok, dec3.words[1].raw_parity
                );
                decoded3 = true;
                break;
            }
        }
        if !decoded3 {
            println!("Perfect symbols also failed! Parity logic bug in make_word or NavDecoder.");
            // Print what NavDecoder sees for HOW
            let mut dec4 = NavDecoder::new();
            for &s in &perfect_syms {
                dec4.feed_symbol(s);
                if dec4.is_synced() && dec4.words.len() == 2 {
                    println!(
                        "After HOW: TLM ok={} HOW ok={}",
                        dec4.words[0].parity_ok, dec4.words[1].parity_ok
                    );
                    println!("  TLM raw_parity=0x{:08X}", dec4.words[0].raw_parity);
                    println!("  HOW raw_parity=0x{:08X}", dec4.words[1].raw_parity);
                    break;
                }
            }
        }
    }
}

/// Real data: track PRN 1, find best bit boundary, feed NavDecoder.
#[test]
fn test_real_data_nav_sync() {
    use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
    use gnss_core::io::{IqFormat, IqReader};
    const DATA_FILE: &str =
        "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
    let spm = FS as usize / 1000;
    let mut reader = IqReader::open(DATA_FILE, IqFormat::Int16).unwrap();
    let total = (30.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.read(&mut signal, total).unwrap();
    let acq_cfg = PcpsConfig {
        fs: FS,
        doppler_max: 10_000.0,
        doppler_step: 250.0,
        threshold: 15.0,
        ..Default::default()
    };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    println!(
        "acq: dopp={:.0} cp={:.2}",
        ar.doppler_hz, ar.code_phase_chips
    );
    // Fine doppler search
    let mut best_dopp = ar.doppler_hz;
    let mut best_qi = f64::MAX;
    for offset in (-250..=250).step_by(50) {
        let init_dopp = ar.doppler_hz + offset as f64;
        let cfg = TrackingConfig {
            fs: FS,
            pll_bw: 50.0,
            pll_order: 3,
            dll_bw: 4.0,
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
    println!("best_dopp={:.0} Q/I={:.3}", best_dopp, best_qi);
    // Track 30s
    let cfg = TrackingConfig {
        fs: FS,
        pll_bw: 50.0,
        pll_order: 3,
        dll_bw: 4.0,
        ..Default::default()
    };
    let mut trk = DllPllTracking::new(0, best_dopp, ar.code_phase_chips, cfg, 0);
    let total_ms = signal.len() / spm;
    let mut prompt_i: Vec<f64> = Vec::with_capacity(total_ms);
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        prompt_i.push(out.prompt.re);
    }
    // Find best boundary by preamble hit count
    let preamble = [1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0];
    let mut best_boundary = 0;
    let mut best_hits = 0;
    println!("\nboundary  hits  best_corr");
    for boundary in 0..20 {
        let mut hits = 0;
        let mut best_corr = 0i32;
        let mut bit_idx = boundary;
        while bit_idx + 8 * 20 <= prompt_i.len() {
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
            if corr.abs() >= 5 {
                hits += 1;
            }
            if corr.abs() > best_corr {
                best_corr = corr.abs();
            }
            bit_idx += 8 * 20;
        }
        if hits > 0 {
            println!("  b{:2}    {}  {}", boundary, hits, best_corr);
        }
        if hits > best_hits {
            best_hits = hits;
            best_boundary = boundary;
        }
    }
    println!("best boundary={} hits={}", best_boundary, best_hits);
    // Print first 30 symbols at best boundary
    println!("\nFirst 30 symbols at boundary {}:", best_boundary);
    for i in 0..30 {
        let start = i * 20 + best_boundary;
        if start + 20 > prompt_i.len() {
            break;
        }
        let sum: f64 = prompt_i[start..start + 20].iter().sum();
        print!("{}({:.0}) ", if sum > 0.0 { "+" } else { "-" }, sum);
        if (i + 1) % 10 == 0 {
            println!();
        }
    }
    println!("\nExpected preamble: + - - - + - + +");
    // Feed NavDecoder at best boundary
    println!("\n=== NavDecoder at boundary {} ===", best_boundary);
    let mut dec = NavDecoder::new();
    for i in 0..prompt_i.len() / 20 {
        let start = i * 20 + best_boundary;
        if start + 20 > prompt_i.len() {
            break;
        }
        let sum: f64 = prompt_i[start..start + 20].iter().sum();
        let was_synced = dec.is_synced();
        let decoded = dec.feed_symbol(sum);
        if dec.is_synced() && !was_synced {
            println!("SYNCED at symbol {}", i);
        }
        if decoded {
            println!("DECODED! sf={} tow={}", dec.subframe_id, dec.tow);
            println!(
                "  TLM parity_ok={} HOW parity_ok={}",
                dec.words[0].parity_ok, dec.words[1].parity_ok
            );
            return;
        }
    }
    println!("Not decoded. Final synced={}", dec.is_synced());
}
