//! M0 verification: PCPS acquisition on synthetic GPS L1 C/A signals.
//!
//! Uses synthetic signals with realistic amplitude + AWGN.
//! Cross-correlation false alarms are expected at high signal powers
//! (real GPS has much higher thermal noise floor).

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

fn gen_signal(prn: usize, doppler: f64, code_phase_chips: f64, fs: f64, amp: f64) -> Vec<Complex<f64>> {
    let n = fs as usize / 1000;
    let spc = fs / 1.023e6;
    let code = &PRN_CODES[prn];
    (0..n)
        .map(|i| {
            let phase = TAU * doppler * i as f64 / fs;
            let chip = code[((i as f64 / spc + code_phase_chips) as usize) % CODE_LENGTH] as f64;
            Complex::new(phase.cos(), phase.sin()) * Complex::new(amp * chip, 0.0)
        })
        .collect()
}

#[test]
fn test_m0_single_strong_signal() {
    let fs = 4_000_000.0;
    let doppler = 1500.0;
    let code_phase = 512.0;
    let amp = 10.0;

    let signal: Vec<Complex<f64>> = gen_signal(5, doppler, code_phase, fs, amp)
        .iter()
        .map(|s| {
            let n_i = fastrand::f64() * 2.0 - 1.0;
            let n_q = fastrand::f64() * 2.0 - 1.0;
            *s + Complex::new(n_i, n_q)
        })
        .collect();

    let config = PcpsConfig {
        fs,
        doppler_max: 5000.0,
        doppler_step: 500.0,
        threshold: 4.0,
        ..Default::default()
    };
    let mut acq = PcpsAcquisition::new(config);
    let r = acq.acquire(5, &signal);

    assert!(r.detected, "PRN 6 should be detected, peak/mean={:.2}", r.peak_to_mean);
    assert_eq!(r.prn, 6);
    assert!((r.doppler_hz - doppler).abs() <= 500.0, "Doppler error: {} Hz", (r.doppler_hz - doppler).abs());
    assert!((r.code_phase_chips - code_phase).abs() < 2.0, "Code phase error: {} chips", (r.code_phase_chips - code_phase).abs());
}

#[test]
fn test_m0_detect_multiple_satellites() {
    let fs = 4_000_000.0;
    let n = fs as usize / 1000;

    // Inject 5 satellites at different Dopplers and code phases
    let injected_prns = [3usize, 7, 15, 22, 28]; // PRN 4, 8, 16, 23, 29
    let dopplers = [-3000.0, -1000.0, 500.0, 2000.0, 4000.0];
    let code_phases = [100.0, 300.0, 500.0, 700.0, 900.0];
    let amp = 8.0;

    let mut signal = vec![Complex::new(0.0, 0.0); n];
    for i in 0..injected_prns.len() {
        let s = gen_signal(injected_prns[i], dopplers[i], code_phases[i], fs, amp);
        for j in 0..n {
            signal[j].re += s[j].re;
            signal[j].im += s[j].im;
        }
    }
    // Add noise
    for sample in &mut signal {
        sample.re += fastrand::f64() * 2.0 - 1.0;
        sample.im += fastrand::f64() * 2.0 - 1.0;
    }

    let config = PcpsConfig {
        fs,
        doppler_max: 5000.0,
        doppler_step: 500.0,
        threshold: 5.0,
        ..Default::default()
    };
    let mut acq = PcpsAcquisition::new(config);

    // For each INJECTED PRN, verify it's detected with correct Doppler within 1 bin
    for i in 0..injected_prns.len() {
        let prn_idx = injected_prns[i];
        let r = acq.acquire(prn_idx, &signal);
        assert!(r.detected,
            "PRN {} should be detected, peak/mean={:.2}",
            r.prn, r.peak_to_mean);
        assert!((r.doppler_hz - dopplers[i]).abs() <= 500.0,
            "PRN {} Doppler error: expected {} got {}",
            r.prn, dopplers[i], r.doppler_hz);
    }
}
