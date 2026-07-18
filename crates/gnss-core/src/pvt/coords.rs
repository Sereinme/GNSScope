//! WGS84 coordinate conversions: ECEF ↔ geodetic (LLA).
//!
//! `ecef_to_lla` uses Bowring's iterative method; `lla_to_ecef` is closed-form.

/// WGS84 ellipsoid parameters.
pub const WGS84_A: f64 = 6_378_137.0;           // semi-major axis [m]
pub const WGS84_F: f64 = 1.0 / 298.257_223_563; // flattening
pub const WGS84_B: f64 = WGS84_A * (1.0 - WGS84_F); // semi-minor axis
pub const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F); // first eccentricity squared

/// Geodetic coordinates: latitude (rad), longitude (rad), height above ellipsoid (m).
pub type Lla = [f64; 3];
/// Earth-Centered Earth-Fixed cartesian coordinates (m).
pub type Ecef = [f64; 3];

/// Geodetic (lat, lon, alt) → ECEF (x, y, z). Closed-form.
pub fn lla_to_ecef(lla: Lla) -> Ecef {
    let (lat, lon, h) = (lla[0], lla[1], lla[2]);
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt(); // prime vertical radius
    let x = (n + h) * cos_lat * lon.cos();
    let y = (n + h) * cos_lat * lon.sin();
    let z = (n * (1.0 - WGS84_E2) + h) * sin_lat;
    [x, y, z]
}

/// ECEF (x, y, z) → geodetic (lat, lon, alt). Bowring's iterative method.
pub fn ecef_to_lla(ecef: Ecef) -> Lla {
    let (x, y, z) = (ecef[0], ecef[1], ecef[2]);
    let lon = y.atan2(x);
    let p = (x * x + y * y).sqrt();
    if p < 1e-12 {
        // pole
        let lat = if z >= 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
        let h = z.abs() - WGS84_B;
        return [lat, lon, h];
    }
    // Bowring iteration
    let mut lat = (z / p).atan2(1.0 - WGS84_E2); // initial guess
    for _ in 0..10 {
        let sin_lat = lat.sin();
        let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();
        let new_lat = (z + WGS84_E2 * n * sin_lat).atan2(p);
        if (new_lat - lat).abs() < 1e-12 {
            lat = new_lat;
            break;
        }
        lat = new_lat;
    }
    let sin_lat = lat.sin();
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();
    let h = p / lat.cos() - n;
    [lat, lon, h]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// CTTC Spain reference position: 41.2747°N, 1.9875°E, alt 52.4 m.
    #[test]
    fn test_cttc_round_trip() {
        let lat = 41.2747_f64.to_radians();
        let lon = 1.9875_f64.to_radians();
        let h = 52.4_f64;
        let ecef = lla_to_ecef([lat, lon, h]);
        let lla = ecef_to_lla(ecef);
        assert!((lla[0] - lat).abs() < 1e-9, "lat diff: {:.2e}", (lla[0] - lat).abs());
        assert!((lla[1] - lon).abs() < 1e-9, "lon diff: {:.2e}", (lla[1] - lon).abs());
        assert!((lla[2] - h).abs() < 1e-4, "alt diff: {:.2e}", (lla[2] - h).abs());
    }

    #[test]
    fn test_known_ecef() {
        // A point on the equator at lon=0, h=0 → x=a, y=0, z=0
        let ecef = lla_to_ecef([0.0, 0.0, 0.0]);
        assert!((ecef[0] - WGS84_A).abs() < 1e-6);
        assert!(ecef[1].abs() < 1e-6);
        assert!(ecef[2].abs() < 1e-6);
    }

    #[test]
    fn test_pole() {
        let ecef = lla_to_ecef([PI / 2.0, 0.0, 0.0]);
        let lla = ecef_to_lla(ecef);
        assert!((lla[0] - PI / 2.0).abs() < 1e-9);
        assert!((lla[2]).abs() < 1e-4);
    }
}
