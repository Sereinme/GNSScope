//! Tracking discriminators for DLL and PLL.
//!
//! Based on GNSS-SDR `tracking_discriminators.h`.

use num_complex::Complex;

/// DLL non-coherent early-minus-late normalized discriminator.
///
/// Returns error in chips. E = √(I_E² + Q_E²), L = √(I_L² + Q_L²).
/// error = (1 - slope) * (E - L) / (E + L + epsilon)
///
/// GNSS-SDR: `dll_nc_e_minus_l_normalized`
pub fn dll_discriminator(early: Complex<f64>, late: Complex<f64>, spc: f64) -> f64 {
    let e_mag = early.norm();
    let l_mag = late.norm();
    let denom = e_mag + l_mag;
    if denom < 1e-30 {
        return 0.0;
    }
    // Normalized: (E - L) / (E + L), scaled to chips
    spc * (e_mag - l_mag) / denom
}

/// PLL Costas loop two-quadrant arctan discriminator.
///
/// Returns phase error in [-π/2, π/2]. Insensitive to 180° nav data flips.
/// GNSS-SDR: `pll_cloop_two_quadrant_atan`
pub fn pll_discriminator(prompt: Complex<f64>) -> f64 {
    // atan(Q/I) limits output to [-π/2, π/2], immune to 180° phase flips
    if prompt.re.abs() < 1e-30 {
        return 0.0;
    }
    f64::atan(prompt.im / prompt.re)
}

/// FLL differential arctan discriminator — exact GNSS-SDR port.
///
/// Returns rad/s. GNSS-SDR: `fll_diff_atan(prompt_s1, prompt_s2, 0, t)`
/// Uses atan(Q/I) (two-quadrant) and phase_unwrap at ±π/2 (Costas).
pub fn fll_discriminator(prompt_prev: Complex<f64>, prompt_curr: Complex<f64>, t: f64) -> f64 {
    let phase_prev = if prompt_prev.re.abs() < 1e-30 {
        0.0
    } else {
        f64::atan(prompt_prev.im / prompt_prev.re)
    };
    let phase_curr = if prompt_curr.re.abs() < 1e-30 {
        0.0
    } else {
        f64::atan(prompt_curr.im / prompt_curr.re)
    };
    let mut diff = phase_curr - phase_prev;
    // GNSS-SDR phase_unwrap: only ±π/2 (Costas loop ambiguity)
    if diff >= std::f64::consts::FRAC_PI_2 {
        diff -= std::f64::consts::PI;
    }
    if diff <= -std::f64::consts::FRAC_PI_2 {
        diff += std::f64::consts::PI;
    }
    diff / t // rad/s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn test_pll_costas_zero_error() {
        // Prompt exactly on I axis → zero phase error
        let p = Complex::new(1.0, 0.0);
        assert!(pll_discriminator(p).abs() < 1e-10);
    }

    #[test]
    fn test_pll_costas_45deg() {
        let p = Complex::new(1.0_f64.sqrt(), 1.0_f64.sqrt());
        let err = pll_discriminator(p);
        assert!((err - PI / 4.0).abs() < 1e-10, "Expected π/4, got {}", err);
    }

    #[test]
    fn test_dll_zero_error() {
        let e = Complex::new(0.5, 0.0);
        let l = Complex::new(0.5, 0.0);
        assert!(dll_discriminator(e, l, 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_dll_early_stronger() {
        let e = Complex::new(0.8, 0.0);
        let l = Complex::new(0.2, 0.0);
        let err = dll_discriminator(e, l, 0.5);
        assert!(err > 0.0, "Early stronger should give positive error");
    }
}
