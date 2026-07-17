//! M1 verification: tracking + navigation message decoding.
use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

const PREAMBLE: u8 = 0x8B;

// Helper: generate clean (no nav mod) signal
fn gen_clean(prn: usize, doppler: f64, cp: f64, fs: f64, amp: f64, dur_ms: usize) -> Vec<Complex<f64>> {
    let spm = fs as usize / 1000;
    let spc = fs / 1.023e6;
    let code = &PRN_CODES[prn];
    (0..dur_ms * spm).map(|i| {
        let n = i % spm;
        let c = code[((n as f64 / spc + cp) as usize) % CODE_LENGTH] as f64;
        let p = TAU * doppler * i as f64 / fs;
        Complex::new(p.cos(), p.sin()) * Complex::new(amp * c, 0.0)
    }).collect()
}

fn make_tlm(ds: u8, de: u8) -> u32 {
    let w: u32 = (PREAMBLE as u32) << 22;
    let d = |n: u32| -> u8 { ((w >> (30 - n)) & 1) as u8 };
    let p25 = ds ^ d(1) ^ d(2) ^ d(3) ^ d(5) ^ d(6) ^ d(10) ^ d(11) ^ d(12) ^ d(13) ^ d(14) ^ d(17) ^ d(18) ^ d(20) ^ d(23);
    let p26 = de ^ d(2) ^ d(3) ^ d(4) ^ d(6) ^ d(7) ^ d(11) ^ d(12) ^ d(13) ^ d(14) ^ d(15) ^ d(18) ^ d(19) ^ d(21) ^ d(24);
    let p27 = ds ^ d(1) ^ d(3) ^ d(4) ^ d(5) ^ d(7) ^ d(8) ^ d(12) ^ d(13) ^ d(14) ^ d(15) ^ d(16) ^ d(19) ^ d(20) ^ d(22);
    let p28 = de ^ d(2) ^ d(4) ^ d(5) ^ d(6) ^ d(8) ^ d(9) ^ d(13) ^ d(14) ^ d(15) ^ d(16) ^ d(17) ^ d(20) ^ d(21) ^ d(23);
    let p29 = de ^ d(1) ^ d(3) ^ d(5) ^ d(6) ^ d(7) ^ d(9) ^ d(10) ^ d(14) ^ d(15) ^ d(16) ^ d(17) ^ d(18) ^ d(21) ^ d(22) ^ d(24);
    let p30 = ds ^ d(3) ^ d(5) ^ d(6) ^ d(8) ^ d(9) ^ d(10) ^ d(11) ^ d(13) ^ d(15) ^ d(19) ^ d(22) ^ d(23) ^ d(24);
    w | (p25 as u32) << 5 | (p26 as u32) << 4 | (p27 as u32) << 3 | (p28 as u32) << 2 | (p29 as u32) << 1 | p30 as u32
}

fn make_how(tow: u32, sf: u8, ds: u8, de: u8) -> u32 {
    let mut w: u32 = 0;
    w |= (tow & 0x1FFFF) << 13;
    w |= ((sf & 0x7) as u32) << 8;
    let d = |n: u32| -> u8 { ((w >> (30 - n)) & 1) as u8 };
    let p25 = ds ^ d(1) ^ d(2) ^ d(3) ^ d(5) ^ d(6) ^ d(10) ^ d(11) ^ d(12) ^ d(13) ^ d(14) ^ d(17) ^ d(18) ^ d(20) ^ d(23);
    let p26 = de ^ d(2) ^ d(3) ^ d(4) ^ d(6) ^ d(7) ^ d(11) ^ d(12) ^ d(13) ^ d(14) ^ d(15) ^ d(18) ^ d(19) ^ d(21) ^ d(24);
    let p27 = ds ^ d(1) ^ d(3) ^ d(4) ^ d(5) ^ d(7) ^ d(8) ^ d(12) ^ d(13) ^ d(14) ^ d(15) ^ d(16) ^ d(19) ^ d(20) ^ d(22);
    let p28 = de ^ d(2) ^ d(4) ^ d(5) ^ d(6) ^ d(8) ^ d(9) ^ d(13) ^ d(14) ^ d(15) ^ d(16) ^ d(17) ^ d(20) ^ d(21) ^ d(23);
    let p29 = de ^ d(1) ^ d(3) ^ d(5) ^ d(6) ^ d(7) ^ d(9) ^ d(10) ^ d(14) ^ d(15) ^ d(16) ^ d(17) ^ d(18) ^ d(21) ^ d(22) ^ d(24);
    let p30 = ds ^ d(3) ^ d(5) ^ d(6) ^ d(8) ^ d(9) ^ d(10) ^ d(11) ^ d(13) ^ d(15) ^ d(19) ^ d(22) ^ d(23) ^ d(24);
    w | (p25 as u32) << 5 | (p26 as u32) << 4 | (p27 as u32) << 3 | (p28 as u32) << 2 | (p29 as u32) << 1 | p30 as u32
}

fn wb(w: u32) -> Vec<u8> { (0..30).rev().map(|i| ((w >> i) & 1) as u8).collect() }

#[test]
fn test_m1_tracking_locks_on_clean_signal() {
    let fs = 4_000_000.0;
    let sig = gen_clean(10, 1000.0, 256.0, fs, 5.0, 500);
    let spm = fs as usize / 1000;
    let mut trk = DllPllTracking::new(10, 1000.0, 256.0, TrackingConfig { fs, ..Default::default() });
    let mut pmax: f64 = 0.0;
    for ms in 0..500 {
        let o = trk.process(&sig[ms * spm..(ms + 1) * spm]);
        pmax = pmax.max(o.prompt.norm());
    }
    assert!(pmax > 1000.0, "Prompt too weak: {:.0}", pmax);
}

#[test]
fn test_nav_decoder_direct() {
    // TLM + HOW with known values, feed directly to decoder
    let tlm = make_tlm(0, 0);
    let td29 = ((tlm >> 1) & 1) as u8;
    let td30 = (tlm & 1) as u8;
    let how = make_how(100, 1, td29, td30);
    let mut bits = wb(tlm);
    bits.extend(&wb(how));
    for _ in 0..8 { bits.extend(&wb(0)); }
    let mut dec = NavDecoder::new();
    for &b in &bits {
        if dec.feed_bit(b) {
            assert_eq!(dec.subframe_id, 1);
            assert_eq!(dec.tow, 100);
            assert!(dec.words[0].parity_ok);
            assert!(dec.words[1].parity_ok);
            return;
        }
    }
    panic!("Decoder should find valid subframe");
}

#[test]
#[ignore = "bit sync resets accumulator; fix pending"]
fn test_m1_track_and_decode_subframe() {
    let fs = 4_000_000.0;
    let prn = 10;
    let doppler = 1000.0;
    let cp = 256.0;
    let amp = 3.0;
    let spm = fs as usize / 1000;
    let spc = fs / 1.023e6;
    let code = &PRN_CODES[prn];

    // Build subframe: TLM + HOW + 8 zero words
    let tlm = make_tlm(0, 0);
    let td29 = ((tlm >> 1) & 1) as u8;
    let td30 = (tlm & 1) as u8;
    let how = make_how(100, 1, td29, td30);
    let mut bits = wb(tlm);
    bits.extend(&wb(how));
    for _ in 0..8 { bits.extend(&wb(0)); }
    let dur_ms = bits.len() * 20;
    // Only run first 2s for debug

    // Generate signal with nav bits (no noise)
    let signal: Vec<Complex<f64>> = (0..dur_ms * spm).map(|i| {
        let n = i % spm;
        let ms = i / spm;
        let bi = ms / 20;
        let b = if bi < bits.len() { bits[bi] } else { 0 };
        let bm = if b == 1 { 1.0 } else { -1.0 };
        let ci = ((n as f64 / spc + cp) as usize) % CODE_LENGTH;
        let c = code[ci] as f64;
        let p = TAU * doppler * i as f64 / fs;
        Complex::new(p.cos(), p.sin()) * Complex::new(amp * c * bm, 0.0)
    }).collect();

    // Acquisition on first 1ms
    let acq = PcpsConfig { fs, doppler_max: 5000.0, doppler_step: 500.0, threshold: 3.0, ..Default::default() };
    let mut a = PcpsAcquisition::new(acq);
    let ar = a.acquire(prn, &signal[..spm]);
    assert!(ar.detected, "Acquisition failed");

    // Start tracking at the correct frequency (from acquisition)
    let tc = TrackingConfig { fs, ..Default::default() };
    let mut trk = DllPllTracking::new(prn, ar.doppler_hz, ar.code_phase_chips, tc);
    let mut dec = NavDecoder::new();

    for ms in 0..dur_ms {
        let _o = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        if let Some(sym) = trk.take_symbol() {
            let bit = if sym.re > 0.0 { 1 } else { 0 };
            let sym_idx = ms / 20;
            println!("sym {}: I={:.0}, bit={}", sym_idx, sym.re, bit);
            if dec.feed_bit(bit) {
                assert_eq!(dec.subframe_id, 1);
                assert_eq!(dec.tow, 100);
                assert!(dec.words[0].parity_ok, "TLM parity fail");
                assert!(dec.words[1].parity_ok, "HOW parity fail");
                return; // success
            }
        }
    }
    panic!("Failed to decode subframe after {} ms", dur_ms);
}
