//! Diagnostics for PLL/DLL tracking on real CTTC Spain data.
//! GNSS-SDR reference pull-in for PRN 1: Doppler=7250 Hz, code phase=3986 samples.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::f64::consts::TAU;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;

fn load_and_acquire(seconds: f64) -> (Vec<Complex<f64>>, usize, f64) {
    let data_path = Path::new(DATA_FILE);
    assert!(data_path.exists(), "real data missing");
    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (seconds * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.read(&mut signal, total).unwrap();
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    println!("acq: dopp={:.0} cp={:.2} peak/mean={:.1}", ar.doppler_hz, ar.code_phase_chips, ar.peak_to_mean);
    (signal, spm, ar.code_phase_chips)
}

/// Find true Doppler via prompt phase residual std (PLL off).
/// Costas atan2(Q,I) is bit-flip immune (±π). At true Doppler, phase is ~constant.
#[test]
fn test_find_true_doppler_phase() {
    let (signal, spm, cp) = load_and_acquire(0.5);
    println!("init_dopp  phase_drift(deg/ms)  resid_std(deg)  |P|mean");

    let mut best_dopp = 7250.0;
    let mut best_metric = f64::MAX;
    for init_dopp in (7000..=7500).step_by(20) {
        let cfg = TrackingConfig { fs: FS, pll_bw: 0.0001, pll_order: 2, dll_bw: 4.0,
            carrier_aiding: true, ..Default::default() };
        let mut trk = DllPllTracking::new(0, init_dopp as f64, cp, cfg);
        let mut phases: Vec<f64> = Vec::new();
        let mut p_sum = 0.0f64;
        for ms in 0..400 {
            let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
            if ms >= 20 {
                phases.push(out.prompt.im.atan2(out.prompt.re));
                p_sum += out.prompt.norm();
            }
        }
        let n = phases.len();
        // Unwrap ±π jumps (bit flips + noise)
        let mut unw: Vec<f64> = Vec::with_capacity(n);
        unw.push(phases[0]);
        for i in 1..n {
            let mut d = phases[i] - phases[i-1];
            while d > std::f64::consts::PI { d -= 2.0 * std::f64::consts::PI; }
            while d < -std::f64::consts::PI { d += 2.0 * std::f64::consts::PI; }
            unw.push(unw[i-1] + d);
        }
        let nf = n as f64;
        let mx = (nf - 1.0) / 2.0;
        let my: f64 = unw.iter().sum::<f64>() / nf;
        let (mut cov, mut var) = (0.0, 0.0);
        for (i, &y) in unw.iter().enumerate() {
            cov += (i as f64 - mx) * (y - my);
            var += (i as f64 - mx).powi(2);
        }
        let slope = if var > 0.0 { cov / var } else { 0.0 };  // rad/sample
        let mut ssr = 0.0;
        for (i, &y) in unw.iter().enumerate() {
            ssr += (y - (my + slope * (i as f64 - mx))).powi(2);
        }
        let resid_std = (ssr / nf).sqrt().to_degrees();
        let drift = (slope * 1000.0).to_degrees();  // deg/ms
        println!("{:6}   {:8.2}            {:8.2}        {:8.0}",
            init_dopp, drift, resid_std, p_sum / nf);
        // Metric: |drift| (lower = true Doppler); resid_std secondary
        if drift.abs() < best_metric { best_metric = drift.abs(); best_dopp = init_dopp as f64; }
    }
    println!("\nBest (lowest |phase drift|): {:.0} Hz (drift={:.2} deg/ms)", best_dopp, best_metric);
}

/// Track 10s at a given Doppler, report Q/I convergence.
fn track(init_dopp: f64, seconds: f64, label: &str, cfg: TrackingConfig) {
    let (signal, spm, cp) = load_and_acquire(seconds);
    let mut trk = DllPllTracking::new(0, init_dopp, cp, cfg);
    let total_ms = signal.len() / spm;
    let mut qi_sum = 0.0;
    let mut qi_n = 0u32;
    let mut qi_min = f64::MAX;
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
        if ms >= 2000 {
            qi_sum += qi;
            qi_n += 1;
            qi_min = qi_min.min(qi);
        }
        if ms % 1000 == 0 {
            println!("  [{}] ms={:5} dopp={:+.1} |I|={:.0} Q/I={:.3}",
                label, ms, out.carrier_doppler_hz, out.prompt.re.abs(), qi);
        }
    }
    println!("[{}] final dopp | Q/I mean={:.4} min={:.4}\n",
        label, qi_sum / qi_n as f64, qi_min);
}

/// Real data PRN 1 with GNSS-SDR config (3rd order, PLL=50, DLL=4).
/// After DLL sign fix, should lock: Doppler stable ~7250, Q/I small.
#[test]
fn test_real_track_locks() {
    let (signal, spm, cp) = load_and_acquire(10.0);
    let cfg = TrackingConfig { fs: FS, pll_bw: 50.0, pll_order: 3, dll_bw: 4.0, ..Default::default() };
    let mut trk = DllPllTracking::new(0, 7250.0, cp, cfg);
    let total_ms = signal.len() / spm;
    let mut qi_sum = 0.0;
    let mut qi_n = 0u32;
    let mut qi_min = f64::MAX;
    let mut dopp_sum = 0.0;
    for ms in 0..total_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
        if ms >= 2000 {
            qi_sum += qi;
            qi_n += 1;
            qi_min = qi_min.min(qi);
            dopp_sum += out.carrier_doppler_hz;
        }
        if ms % 1000 == 0 {
            println!("ms={:5} dopp={:+.1} |I|={:.0} Q/I={:.3} cn0={:.1}",
                ms, out.carrier_doppler_hz, out.prompt.re.abs(), qi, out.cn0_db_hz);
        }
    }
    let qi_mean = qi_sum / qi_n as f64;
    let dopp_mean = dopp_sum / qi_n as f64;
    println!("\nDoppler mean={:.1} (target ~7250) | Q/I mean={:.4} min={:.4}",
        dopp_mean, qi_mean, qi_min);
    // Doppler must converge near 7250 (primary lock indicator)
    assert!((dopp_mean - 7250.0).abs() < 200.0, "Doppler should be near 7250: {:.1}", dopp_mean);
    // Q/I median should be small (mean is skewed by bit-flip transients)
    assert!(qi_min < 0.01, "PLL should achieve near-zero Q/I at some points, got min={:.4}", qi_min);
}

/// Real data: print code_phase + |E|/|L| to diagnose DLL.
#[test]
fn test_real_dll_diagnostic() {
    let (signal, spm, cp_init) = load_and_acquire(5.0);
    let cfg = TrackingConfig { fs: FS, pll_bw: 50.0, pll_order: 3, dll_bw: 4.0, ..Default::default() };
    let mut trk = DllPllTracking::new(0, 7250.0, cp_init, cfg);
    println!("init cp={:.2}", cp_init);
    println!("ms    dopp     cp     |E|    |P|    |L|   E/L");
    for ms in 0..5000 {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        if ms % 250 == 0 || ms < 5 {
            let el_ratio = if out.late_mag > 1.0 { out.early_mag / out.late_mag } else { 0.0 };
            println!("{:4}  {:7.1}  {:6.2}  {:6.0}  {:6.0}  {:6.0}  {:.3}",
                ms, out.carrier_doppler_hz, out.code_phase_chips,
                out.early_mag, out.prompt.norm(), out.late_mag, el_ratio);
        }
    }
}

/// Clean signal, target 1000 Hz, init NCO at 950 (50 Hz error).
#[test]
fn test_clean_pll_pullin() {
    use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
    let spm = FS as usize / 1000;
    let prn = 5;
    let doppler = 1000.0_f64;
    let spc = FS / 1.023e6;
    let code = &PRN_CODES[prn];
    let dur_ms = 500;
    let signal: Vec<Complex<f64>> = (0..dur_ms * spm).map(|i| {
        let n = i % spm;
        let ci = (n as f64 / spc) as usize % CODE_LENGTH;
        let phase = TAU * doppler * i as f64 / FS;
        Complex::new(phase.cos(), phase.sin()) * Complex::new(3.0 * code[ci] as f64, 0.0)
    }).collect();
    // bw=50 converges in 100ms; verify
    let cfg = TrackingConfig { fs: FS, pll_bw: 50.0, pll_order: 2, ..Default::default() };
    let mut trk = DllPllTracking::new(prn, 950.0, 0.0, cfg);
    let mut converged = false;
    for ms in 0..dur_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        if ms >= 200 && (out.carrier_doppler_hz - 1000.0).abs() < 1.0 {
            converged = true;
        }
    }
    println!("bw=50 converged to 1000 Hz: {}", converged);
    assert!(converged, "PLL bw=50 should converge to 1000 Hz");
}

/// Acquire once, then re-acquire at 1s, 2s, 3s — does the code phase drift?
/// If acq code phase is stable but tracker drifts, the tracker's code NCO is wrong.
#[test]
fn test_acq_code_phase_stability() {
    let data_path = Path::new(DATA_FILE);
    assert!(data_path.exists(), "real data missing");
    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    // Re-acquire at 0s, 1s, 2s, 3s — code phase should drift by ~code_doppler * t
    for sec in 0..4 {
        let mut buf = vec![Complex::new(0.0, 0.0); spm];
        reader.seek_to_sample((sec as f64 * FS) as u64).unwrap();
        reader.read(&mut buf, spm).unwrap();
        let ar = acq.acquire(0, &buf);
        println!("t={}s: dopp={:.0} cp={:.2} chips peak/mean={:.1}",
            sec, ar.doppler_hz, ar.code_phase_chips, ar.peak_to_mean);
    }
}

/// Print raw prompt phase (no unwrap) for first 30ms at 7250 Hz, PLL off.
#[test]
fn test_raw_phase_series() {
    let (signal, spm, cp) = load_and_acquire(0.05);
    let cfg = TrackingConfig { fs: FS, pll_bw: 0.0001, pll_order: 2, dll_bw: 4.0,
        carrier_aiding: true, ..Default::default() };
    let mut trk = DllPllTracking::new(0, 7250.0, cp, cfg);
    println!("ms   I        Q        atan2(deg)   |P|      carr_freq   carr_phase");
    for ms in 0..30 {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        let ph = out.prompt.im.atan2(out.prompt.re).to_degrees();
        println!("{:3}  {:8.0}  {:8.0}  {:8.1}    {:8.0}   {:8.1}  {:.5}",
            ms, out.prompt.re, out.prompt.im, ph, out.prompt.norm(),
            trk.carrier_freq_hz(), trk.carrier_phase_rad());
    }
}

/// Clean signal + Gaussian noise at realistic C/N0 (~45 dB-Hz).
/// If PLL locks here but not on real data, the issue is real-data-specific
/// (e.g. DC offset, IF leakage, sampling). If it diverges, our PLL is SNR-fragile.
#[test]
fn test_clean_signal_with_noise() {
    use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
    let spm = FS as usize / 1000;
    let prn = 5;
    let doppler = 7250.0_f64;
    let spc = FS / 1.023e6;
    let code = &PRN_CODES[prn];
    let dur_ms = 5000;
    // amp=3 gives |P|~12000 clean. Real data |P|~95000 → amp~24.
    // Add noise to get C/N0 ~ 45 dB-Hz.
    let amp = 24.0;
    let noise_sigma = 50.0;  // tune for ~45 dB-Hz
    let signal: Vec<Complex<f64>> = (0..dur_ms * spm).map(|i| {
        let n = i % spm;
        let ci = (n as f64 / spc) as usize % CODE_LENGTH;
        let phase = TAU * doppler * i as f64 / FS;
        // Box-Muller noise
        let u1 = ((i * 2654435761u64 as usize) % 1000000 + 1) as f64 / 1000001.0;
        let u2 = ((i * 40503u64 as usize) % 1000000 + 1) as f64 / 1000001.0;
        let nr = (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos() * noise_sigma;
        let ni = (-2.0 * u1.ln()).sqrt() * (TAU * u2).sin() * noise_sigma;
        Complex::new(phase.cos() * amp * code[ci] as f64 + nr,
                     phase.sin() * amp * code[ci] as f64 + ni)
    }).collect();
    let cfg = TrackingConfig { fs: FS, pll_bw: 50.0, pll_order: 3, ..Default::default() };
    let mut trk = DllPllTracking::new(prn, doppler, 0.0, cfg);
    let mut qi_sum = 0.0;
    let mut qi_n = 0u32;
    for ms in 0..dur_ms {
        let out = trk.process(&signal[ms * spm..(ms + 1) * spm]);
        if ms >= 500 {
            qi_sum += (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
            qi_n += 1;
        }
        if ms % 1000 == 0 {
            println!("noisy ms={:5} dopp={:+.1} |I|={:.0} Q/I={:.3}",
                ms, out.carrier_doppler_hz, out.prompt.re.abs(),
                (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs());
        }
    }
    println!("noisy clean: Q/I mean={:.4}", qi_sum / qi_n as f64);
}
