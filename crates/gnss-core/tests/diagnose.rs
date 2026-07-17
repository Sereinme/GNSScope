//! Deep phase diagnostic: scan 20 bit boundaries + monitor PLL Q/I over time.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;
const PREAMBLE: [f64; 8] = [1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0];

#[test]
fn diagnose_phase() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() { eprintln!("SKIP"); return; }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    let total = (10.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.read(&mut signal, total).unwrap();

    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);
    let ar = acq.acquire(0, &signal[..spm]);
    assert!(ar.detected);
    println!("Acq: doppler={:.0}, code_phase={:.1}", ar.doppler_hz, ar.code_phase_chips);

    // Track for first 1s with detailed PLL output
    let mut trk2 = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, 
        TrackingConfig { fs: FS, coherent_ms: 1, ..Default::default() });
    
    println!("\nDetailed PLL/FLL (first 1000ms):");
    println!("  ms    pll_in    fll_in     Doppler   |I|     |Q|");
    for ms in 0..1000.min(total_ms) {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        let out = trk2.process(chunk);
        if ms % 100 == 0 {
            // We can't get pll_in/fll_in directly, but we can see Doppler and Q/I
            println!("  {:4} {:8.1} {:8.0} {:8.0}",
                ms, out.carrier_doppler_hz,
                out.prompt.re.abs(), out.prompt.im.abs());
        }
    }

    // Track with boundary 18 (found best) for 2s, check preamble corr
    println!("\n--- Boundary 18 preamble corr over time ---");
    let mut trk3 = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips,
        TrackingConfig { fs: FS, coherent_ms: 1, ..Default::default() });
    let total_ms3 = (2.0 * FS) as usize / spm;
    for ms in 0..total_ms3 {
        trk3.process(&signal[ms * spm..(ms + 1) * spm]);
    }
    // Now collect 1ms prompts from trk3 (we need a different approach — 
    // trk3 has already run. We need to re-accumulate.)
    // Skip for now, focus on PLL lock.
    let trk_cfg = TrackingConfig { fs: FS, coherent_ms: 1, ..Default::default() };
    let mut trk = DllPllTracking::new(0, ar.doppler_hz, ar.code_phase_chips, trk_cfg);
    let total_ms = total / spm;
    let mut prompt_i: Vec<f64> = Vec::with_capacity(total_ms);
    let mut prompt_q: Vec<f64> = Vec::with_capacity(total_ms);
    let mut dopplers: Vec<f64> = Vec::with_capacity(total_ms);

    for ms in 0..total_ms {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        let out = trk.process(chunk);
        prompt_i.push(out.prompt.re);
        prompt_q.push(out.prompt.im);
        dopplers.push(out.carrier_doppler_hz);
    }

    // --- PLL quality over time ---
    println!("\nTime  Doppler   |I_1ms|   |Q_1ms|   Q/I");
    for t in (0..total_ms).step_by(500) {
        let (mut i_sum, mut q_sum) = (0.0f64, 0.0f64);
        let w = 200.min(total_ms - t);
        for k in 0..w {
            i_sum += prompt_i[t + k].abs();
            q_sum += prompt_q[t + k].abs();
        }
        println!("{:5}ms {:8.1} {:8.0} {:8.0} {:7.3}",
            t, dopplers[t], i_sum/w as f64, q_sum/w as f64,
            (q_sum / (i_sum + 1e-30)));
    }

    // --- Scan 20 boundaries for preamble correlation ---
    println!("\n--- Preamble corr at 20 boundaries (first 200 bits) ---");
    for boundary in 0..20 {
        let mut corrs: Vec<f64> = Vec::new();
        let mut bit_start = boundary;
        while bit_start + 8 * 20 <= prompt_i.len() && bit_start + 8 * 20 <= 200 * 20 + boundary {
            // Compute 20ms symbols
            let mut syms = [0.0f64; 8];
            for b in 0..8 {
                let s = bit_start + b * 20;
                let e = s + 20;
                syms[b] = prompt_i[s..e].iter().sum();
            }
            // Preamble correlation
            let mut corr = 0.0f64;
            for b in 0..8 {
                corr += syms[b].signum() * PREAMBLE[b];
            }
            corrs.push(corr);
            bit_start += 8 * 20;
        }
        let best = corrs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let avg: f64 = corrs.iter().sum::<f64>() / corrs.len() as f64;
        println!("  b{:2}: best={:4.0} avg={:6.1} n={}", boundary, best, avg, corrs.len());
    }
}
