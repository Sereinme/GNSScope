//! Satellite position computation from ephemeris (Keplerian orbit).
//!
//! Port of RTKLIB/GNSS-SDR `eph2pos`. Computes ECEF position and clock
//! correction at a given GPS transmit time.

use crate::nav::ephemeris::Ephemeris;
use crate::pvt::coords::Ecef;

// Physical constants (GPS)
const MU_GPS: f64 = 3.986_005e14; // Earth gravitational constant [m^3/s^2]
const OMEGA_E: f64 = 7.292_115_146_7e-5; // Earth rotation rate [rad/s]
const C_LIGHT: f64 = 299_792_458.0; // speed of light [m/s]

#[derive(Debug, Clone, Copy)]
pub struct SatPos {
    pub ecef: Ecef,
    pub clock_correction_s: f64,
    /// Eccentric anomaly E (for velocity / relativistic use).
    pub eccentric_anomaly: f64,
}

/// Compute satellite ECEF position + clock correction at GPS system time `t`
/// (seconds of week). `t` is the *transmit* time corrected for satellite clock.
pub fn satellite_position(eph: &Ephemeris, t: f64) -> SatPos {
    let a = eph.sqrt_a * eph.sqrt_a; // semi-major axis
    let n0 = (MU_GPS / (a * a * a)).sqrt(); // computed mean motion
    let tk = gps_time_diff(t, eph.toe); // time from ephemeris ref
    let n = n0 + eph.delta_n; // corrected mean motion
    let mk = eph.m0 + n * tk; // mean anomaly

    // Solve Kepler's equation: M = E - e·sin(E). Newton iteration.
    let mut e = mk;
    for _ in 0..30 {
        let delta = (e - eph.eccentricity * e.sin() - mk) / (1.0 - eph.eccentricity * e.cos());
        e -= delta;
        if delta.abs() < 1e-13 {
            break;
        }
    }
    let ek = e;

    // True anomaly
    let sin_e = ek.sin();
    let cos_e = ek.cos();
    let nu = ((1.0 - eph.eccentricity * eph.eccentricity).sqrt() * sin_e)
        .atan2(cos_e - eph.eccentricity);
    let phi = nu + eph.omega; // argument of latitude

    // Second harmonic perturbations
    let two_phi = 2.0 * phi;
    let sin_2phi = two_phi.sin();
    let cos_2phi = two_phi.cos();
    let u = phi + eph.cus * sin_2phi + eph.cuc * cos_2phi; // corrected arg of latitude
    let r = a * (1.0 - eph.eccentricity * cos_e) + eph.crs * sin_2phi + eph.crc * cos_2phi; // radius
    let i = eph.i0 + eph.i_dot * tk + eph.cis * sin_2phi + eph.cic * cos_2phi; // inclination

    // Positions in orbital plane
    let x_op = r * u.cos();
    let y_op = r * u.sin();

    // Corrected longitude of ascending node (with Earth rotation)
    let omega = eph.omega0 + (eph.omega_dot - OMEGA_E) * tk - OMEGA_E * eph.toe;
    let cos_om = omega.cos();
    let sin_om = omega.sin();
    let cos_i = i.cos();
    let sin_i = i.sin();

    let x = x_op * cos_om - y_op * cos_i * sin_om;
    let y = x_op * sin_om + y_op * cos_i * cos_om;
    let z = y_op * sin_i;

    // Satellite clock correction (with relativistic term)
    let tk_clock = gps_time_diff(t, eph.toc);
    let dt_rel = -2.0 * (MU_GPS * a).sqrt() * eph.eccentricity * sin_e / (C_LIGHT * C_LIGHT);
    let dt_sv = eph.af0 + eph.af1 * tk_clock + eph.af2 * tk_clock * tk_clock + dt_rel - eph.tgd;

    SatPos {
        ecef: [x, y, z],
        clock_correction_s: dt_sv,
        eccentric_anomaly: ek,
    }
}

/// Satellite velocity (ECEF, m/s) via analytical derivatives. Useful for
/// Doppler-based velocity solution (not required for M2 position-only, but
/// cheap to derive).
pub fn satellite_velocity(eph: &Ephemeris, t: f64) -> Ecef {
    let a = eph.sqrt_a * eph.sqrt_a;
    let n0 = (MU_GPS / (a * a * a)).sqrt();
    let tk = gps_time_diff(t, eph.toe);
    let n = n0 + eph.delta_n;
    let mk = eph.m0 + n * tk;
    let mut e = mk;
    for _ in 0..30 {
        let delta = (e - eph.eccentricity * e.sin() - mk) / (1.0 - eph.eccentricity * e.cos());
        e -= delta;
        if delta.abs() < 1e-13 {
            break;
        }
    }
    let ek = e;
    let sin_e = ek.sin();
    let cos_e = ek.cos();
    let nu = ((1.0 - eph.eccentricity * eph.eccentricity).sqrt() * sin_e)
        .atan2(cos_e - eph.eccentricity);
    let phi = nu + eph.omega;
    let two_phi = 2.0 * phi;
    let sin_2phi = two_phi.sin();
    let cos_2phi = two_phi.cos();
    let u = phi + eph.cus * sin_2phi + eph.cuc * cos_2phi;
    let r = a * (1.0 - eph.eccentricity * cos_e) + eph.crs * sin_2phi + eph.crc * cos_2phi;
    let i = eph.i0 + eph.i_dot * tk + eph.cis * sin_2phi + eph.cic * cos_2phi;
    let omega = eph.omega0 + (eph.omega_dot - OMEGA_E) * tk - OMEGA_E * eph.toe;

    // Derivatives
    let edot = n / (1.0 - eph.eccentricity * cos_e);
    let phidot = edot * (1.0 + eph.eccentricity * cos_e)
        / (1.0 - eph.eccentricity * eph.eccentricity).sqrt();
    // (omitting harmonic derivative terms for brevity — small contribution)
    let rdot = a * eph.eccentricity * sin_e * edot;
    let udot = phidot;
    let idot = eph.i_dot;
    let omegadot = eph.omega_dot - OMEGA_E;

    let x_op = r * u.cos();
    let y_op = r * u.sin();
    let vx_op = rdot * u.cos() - r * u.sin() * udot;
    let vy_op = rdot * u.sin() + r * u.cos() * udot;

    let cos_om = omega.cos();
    let sin_om = omega.sin();
    let cos_i = i.cos();
    let sin_i = i.sin();
    let vx = vx_op * cos_om - y_op * cos_i * sin_om * omegadot + x_op * (-sin_om) * omegadot
        - vy_op * cos_i * sin_om
        + y_op * sin_i * sin_om * idot;
    let vy = vx_op * sin_om
        + y_op * cos_i * cos_om * omegadot
        + x_op * cos_om * omegadot
        + vy_op * cos_i * cos_om
        - y_op * sin_i * cos_om * idot;
    let vz = vy_op * sin_i + y_op * cos_i * idot;
    [vx, vy, vz]
}

/// Difference t - ref, accounting for week crossover (±302400 s).
fn gps_time_diff(t: f64, ref_t: f64) -> f64 {
    let mut dt = t - ref_t;
    if dt > 302_400.0 {
        dt -= 604_800.0;
    } else if dt < -302_400.0 {
        dt += 604_800.0;
    }
    dt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kepler_converges() {
        // Minimal ephemeris: circular orbit, e=0, no perturbations.
        let eph = Ephemeris {
            sqrt_a: 26_559_710.0_f64.sqrt(), // ~semi-major axis 26560 km
            eccentricity: 0.01,
            m0: 0.0,
            omega: 0.0,
            omega0: 0.0,
            omega_dot: 0.0,
            i0: 0.55,
            i_dot: 0.0,
            delta_n: 0.0,
            cus: 0.0,
            cuc: 0.0,
            crs: 0.0,
            crc: 0.0,
            cis: 0.0,
            cic: 0.0,
            toe: 0.0,
            toc: 0.0,
            af0: 0.0,
            af1: 0.0,
            af2: 0.0,
            tgd: 0.0,
            ..Default::default()
        };
        let pos = satellite_position(&eph, 100.0);
        let r = (pos.ecef[0].powi(2) + pos.ecef[1].powi(2) + pos.ecef[2].powi(2)).sqrt();
        // Orbital radius should be near a (26560 km), within harmonic-free bounds.
        assert!(r > 26_000_000.0 && r < 27_000_000.0, "r = {}", r);
    }

    #[test]
    fn test_time_diff_week_crossover() {
        // 600_000 - 100_000 = 500_000 > 302_400 → wrap: 500_000 - 604_800 = -104_800
        assert!((gps_time_diff(600_000.0, 100_000.0) - (-104_800.0)).abs() < 1e-6);
        // 0 - 600_000 = -600_000 < -302_400 → wrap: -600_000 + 604_800 = 4_800
        assert!((gps_time_diff(0.0, 600_000.0) - 4_800.0).abs() < 1e-6);
        assert!((gps_time_diff(100.0, 50.0) - 50.0).abs() < 1e-6);
    }
}
