//! Sample rate conversion utilities.

use num_complex::Complex;

/// Decimate complex samples by factor 2 using averaging (anti-alias filter).
/// Input rate → output rate = input_rate / 2.
pub fn decimate_by_2(samples: &[Complex<f64>]) -> Vec<Complex<f64>> {
    let n = samples.len() / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let s = samples[i * 2] + samples[i * 2 + 1];
        out.push(s * 0.5); // average
    }
    out
}
