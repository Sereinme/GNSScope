//! Weighted Least Squares (WLS) position solver.
//!
//! Iteratively linearizes pseudorange residuals around the current receiver
//! estimate and solves the normal equations. Weights derived from per-channel
//! C/N₀. State: [x, y, z, clk_bias] (4 unknowns).

use crate::nav::ephemeris::{EphemerisMap, IonoParams};
use crate::pvt::atmosphere::{klobuchar_iono, saastamoinen_trop};
use crate::pvt::coords::{ecef_to_lla, Ecef};
use crate::pvt::pseudorange::Pseudorange;
use crate::pvt::satpos::satellite_position;
use nalgebra::{DMatrix, DVector, Matrix4, Vector4};

const C_LIGHT: f64 = 299_792_458.0;
const OMEGA_E: f64 = 7.292_115_146_7e-5;
const MAX_ITER: usize = 10;
const CONV_TOL: f64 = 1e-4; // [m]
const MAX_SATS: usize = 12;

/// PVT solution (position-only for M2; velocity is a future extension).
#[derive(Debug, Clone)]
pub struct PvtSolution {
    pub ecef: Ecef,
    pub lla: [f64; 3], // [lat_rad, lon_rad, alt_m]
    pub clock_bias_s: f64,
    pub n_sats: usize,
    /// Per-satellite post-fit residual [m].
    pub residuals_m: Vec<f64>,
    pub converged: bool,
}

impl Default for PvtSolution {
    fn default() -> Self {
        Self {
            ecef: [0.0; 3],
            lla: [0.0; 3],
            clock_bias_s: 0.0,
            n_sats: 0,
            residuals_m: Vec::new(),
            converged: false,
        }
    }
}

/// Solve receiver position from pseudoranges and ephemerides.
///
/// - `pseudoranges`: one per satellite (≥4 required).
/// - `ephemerides`: PRN → validated Ephemeris.
/// - `iono`: Klobuchar parameters (may be invalid if no subframe-4 decoded yet).
///
/// `rx_lat_rad`, `rx_lon_rad`, `rx_alt_m` are approximate a-priori coordinates
/// (for iono/tropo); pass zeros to bootstrap from Earth center + iteration.
pub fn solve_wls(
    pseudoranges: &[Pseudorange],
    ephemerides: &EphemerisMap,
    iono: &IonoParams,
    rx_lat_rad: f64,
    rx_lon_rad: f64,
    rx_alt_m: f64,
) -> PvtSolution {
    let n = pseudoranges.len();
    if n < 4 {
        return PvtSolution { n_sats: n, ..Default::default() };
    }
    let n = n.min(MAX_SATS);

    // Initial estimate: Earth center, zero clock bias.
    let mut x = Vector4::<f64>::zeros(); // [x, y, z, clk_bias_m]

    let mut last_residuals: Vec<f64> = Vec::new();
    let mut used_count = 0usize;
    let mut converged = false;

    for iter in 0..MAX_ITER {
        let rx_pos: Ecef = [x[0], x[1], x[2]];
        let (lat_r, lon_r, alt_r) = if iter == 0 {
            (rx_lat_rad, rx_lon_rad, rx_alt_m)
        } else {
            let lla = ecef_to_lla(rx_pos);
            (lla[0], lla[1], lla[2])
        };

        // Build residual vector v (n×1) and design matrix H (n×4) with weights.
        let mut v = DVector::<f64>::zeros(n);
        let mut h = DMatrix::<f64>::zeros(n, 4);
        let mut w_diag = DVector::<f64>::zeros(n);
        let mut used = 0usize;

        for pr in pseudoranges.iter().take(MAX_SATS) {
            let eph = match ephemerides.get(&pr.prn) {
                Some(e) => e,
                None => continue,
            };
            // Satellite position at transmit time (already GPS system time).
            let t_sv = pr.transmit_time_sow;
            let sat = satellite_position(eph, t_sv);

            // Sagnac (Earth rotation) correction: rotate satellite ECEF from the
            // transmit frame to the receive frame.
            let tau = pr.pseudorange_m / C_LIGHT;
            let theta = OMEGA_E * tau;
            let cos_th = theta.cos();
            let sin_th = theta.sin();
            let xs = sat.ecef[0];
            let ys = sat.ecef[1];
            let zs = sat.ecef[2];
            let xs_r = cos_th * xs + sin_th * ys;
            let ys_r = -sin_th * xs + cos_th * ys;
            let zs_r = zs;

            // Geometric range + line-of-sight unit vector
            let dx = xs_r - rx_pos[0];
            let dy = ys_r - rx_pos[1];
            let dz = zs_r - rx_pos[2];
            let range = (dx * dx + dy * dy + dz * dz).sqrt();
            if range < 1.0 { continue; }
            let ex = dx / range;
            let ey = dy / range;
            let ez = dz / range;

            // Azimuth / elevation for atmospheric corrections
            let (az, el) = azel(rx_pos, [xs_r, ys_r, zs_r]);

            // Atmospheric delays
            let iono_m = if iono.valid {
                klobuchar_iono(iono, lat_r, lon_r, az, el, t_sv)
            } else { 0.0 };
            let trop_m = saastamoinen_trop(lat_r, alt_r, el);

            // Corrected pseudorange (remove sat clock, add atmosphere)
            let pr_corrected = pr.pseudorange_m
                + C_LIGHT * sat.clock_correction_s
                - iono_m
                - trop_m;

            // Residual: measured - predicted
            let predicted = range + x[3]; // + receiver clock bias (in meters)
            let resid = pr_corrected - predicted;

            // Weight from C/N0: higher C/N0 → smaller variance.
            let cn0 = pr.cn0_db_hz.clamp(10.0, 60.0);
            let weight = 10.0_f64.powf((cn0 - 30.0) / 10.0); // ~1 at 30 dB-Hz

            v[used] = resid;
            h[(used, 0)] = -ex;
            h[(used, 1)] = -ey;
            h[(used, 2)] = -ez;
            h[(used, 3)] = 1.0;
            w_diag[used] = weight;
            used += 1;
        }

        if used < 4 {
            return PvtSolution { n_sats: used, ..Default::default() };
        }

        // Truncate to `used` rows.
        let v = v.rows(0, used).clone_owned();
        let h = h.rows(0, used).clone_owned();
        let w_diag = w_diag.rows(0, used).clone_owned();
        let w = DMatrix::from_diagonal(&w_diag);

        // Normal equations: dx = (Hᵀ W H)⁻¹ Hᵀ W v
        let htw = &h.transpose() * &w;
        let htw = &htw * &h;
        // HᵀWH is always 4×4 (4 unknowns: x,y,z,clk).
        let htw4: Matrix4<f64> = htw.fixed_resize::<4, 4>(0.0);
        let htwv = &h.transpose() * &w * &v;
        let htwv4: Vector4<f64> = htwv.fixed_resize::<4, 1>(0.0);

        let dx = match htw4.try_inverse() {
            Some(inv) => inv * htwv4,
            None => return PvtSolution { n_sats: used, ..Default::default() },
        };

        x[0] += dx[0];
        x[1] += dx[1];
        x[2] += dx[2];
        x[3] += dx[3];

        last_residuals = (0..used).map(|i| v[i]).collect();
        used_count = used;

        let step = dx.rows(0, 3).norm();
        if step < CONV_TOL {
            converged = true;
            break;
        }
    }

    let ecef: Ecef = [x[0], x[1], x[2]];
    let lla = ecef_to_lla(ecef);

    PvtSolution {
        ecef,
        lla,
        clock_bias_s: x[3] / C_LIGHT,
        n_sats: used_count,
        residuals_m: last_residuals,
        converged,
    }
}

/// Compute azimuth and elevation of a satellite (ECEF) from a receiver (ECEF).
fn azel(rx: Ecef, sat: Ecef) -> (f64, f64) {
    let lla = ecef_to_lla(rx);
    let lat = lla[0];
    let lon = lla[1];

    let dx = sat[0] - rx[0];
    let dy = sat[1] - rx[1];
    let dz = sat[2] - rx[2];

    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let sin_lon = lon.sin();
    let cos_lon = lon.cos();

    let e = -sin_lon * dx + cos_lon * dy;
    let n = -sin_lat * cos_lon * dx - sin_lat * sin_lon * dy + cos_lat * dz;
    let u = cos_lat * cos_lon * dx + cos_lat * sin_lon * dy + sin_lat * dz;

    let az = e.atan2(n);
    let horiz = (e * e + n * n).sqrt();
    let el = (u / horiz.max(1e-12)).atan();
    (az, el)
}
