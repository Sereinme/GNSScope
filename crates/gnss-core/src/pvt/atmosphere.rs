//! Atmospheric delay models: Klobuchar ionosphere and Saastamoinen troposphere.
//!
//! Both return delay in meters. Ported from RTKLIB `ionmodel` / `tropmodel`.

use crate::nav::ephemeris::IonoParams;

const C_LIGHT: f64 = 299_792_458.0;

/// Klobuchar ionospheric delay [m]. Standard 8-parameter α/β model.
///
/// Inputs:
/// - `iono`: broadcast α/β coefficients
/// - `lat`, `lon`: receiver geodetic latitude/longitude [rad]
/// - `az`, `el`: satellite azimuth/elevation [rad]
/// - `gpst`: GPS seconds of week [s]
pub fn klobuchar_iono(iono: &IonoParams, lat: f64, lon: f64, az: f64, el: f64, gpst: f64) -> f64 {
    // Semi-circle convention (per IS-GPS-200)
    let phii = lat / std::f64::consts::PI; // user lat [sc]
    let lambdai = lon / std::f64::consts::PI; // user lon [sc]
    let a = az / std::f64::consts::PI; // azimuth [sc]
    let e = el / std::f64::consts::PI; // elevation [sc]

    // Earth-centred angle [sc]
    let psi = 0.0137 / (e + 0.11) - 0.022;

    // Sub-ionospheric latitude [sc], clamp ±0.416
    let phiu = phii + psi * a.cos();
    let phiu = phiu.clamp(-0.416, 0.416);

    // Sub-ionospheric longitude [sc]
    let lambdau = lambdai + psi * a.sin() / phiu.cos();

    // Geomagnetic latitude [sc]
    let phim = phiu + 0.064 * ((lambdau - 1.617) * std::f64::consts::PI).cos();

    // Local time [s]
    let mut t = 43_200.0 * lambdau + gpst;
    while t < 0.0 { t += 86_400.0; }
    while t >= 86_400.0 { t -= 86_400.0; }

    // Slant factor
    let f = 1.0 + 16.0 * (0.53 - e).powi(3);

    // Amplitude [s]
    let mut amp = iono.alpha[0] + phim * (iono.alpha[1] + phim * (iono.alpha[2] + phim * iono.alpha[3]));
    if amp < 0.0 { amp = 0.0; }

    // Period [s]
    let mut per = iono.beta[0] + phim * (iono.beta[1] + phim * (iono.beta[2] + phim * iono.beta[3]));
    if per < 72_000.0 { per = 72_000.0; }

    // Phase [radians]
    let x = 2.0 * std::f64::consts::PI * (t - 50_400.0) / per;

    // Delay [s]: half-cosine if |x| < π/2, else constant 5 ns term
    let t_iono = if x.abs() < std::f64::consts::FRAC_PI_2 {
        5e-9 + f * (5e-9 + amp * x.cos())
    } else {
        f * 5e-9
    };

    t_iono * C_LIGHT
}

/// Saastamoinen tropospheric delay [m]. Standard atmosphere (P, T, RH hardcoded
/// per CONTEXT.md; GUI-configurable later).
///
/// `lat`, `h`: receiver geodetic latitude [rad], height above ellipsoid [m].
/// `el`: satellite elevation [rad].
pub fn saastamoinen_trop(lat: f64, h: f64, el: f64) -> f64 {
    // Standard meteorological parameters
    let p0 = 1013.25; // sea-level pressure [hPa]
    let t0 = 293.15;  // sea-level temperature [K] (~20°C)
    let rh = 0.5;     // relative humidity

    // Tropospheric model per RTKLIB tropmodel:
    let hgt = h; // height above ellipsoid [m]
    let _ = lat;

    // Pressure/temperature at receiver height (simple lapse)
    let pres = p0 * (1.0 - 2.2557e-5 * hgt).powf(5.2568);
    let temp = t0 - 6.5e-3 * hgt;
    let e0 = rh * 6.108 * ((17.15 * temp - 4684.0) / (temp - 38.45)).exp();

    // Zenith wet delay
    let zwd = 0.002_277 * (1255.0 / temp + 0.05) * e0;
    // Zenith hydrostatic delay
    let zhd = 0.002_277 * pres;

    // Mapping function (simple 1/sin(el))
    let sin_el = el.sin();
    if sin_el < 0.1 { return (zhd + zwd) * 10.0; } // cap at low elevation
    (zhd + zwd) / sin_el
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_klobuchar_typical_range() {
        let iono = IonoParams {
            alpha: [1.0e-8, 1.0e-9, -5.0e-9, 1.0e-10],
            beta: [1.0e5, 1.0e5, -1.0e5, 1.0e5],
            valid: true,
        };
        // Mid-latitude, moderate elevation, midday
        let lat = 41.0_f64.to_radians();
        let lon = 2.0_f64.to_radians();
        let az = 45.0_f64.to_radians();
        let el = 45.0_f64.to_radians();
        let gpst = 43_200.0; // noon
        let delay = klobuchar_iono(&iono, lat, lon, az, el, gpst);
        // Typical Klobuchar delay: 1–20 m at zenith, scaled by slant factor.
        assert!(delay > 0.0 && delay < 100.0, "delay = {}", delay);
    }

    #[test]
    fn test_saastamoinen_zenith() {
        let lat = 41.0_f64.to_radians();
        let h = 50.0;
        let el = std::f64::consts::FRAC_PI_2; // zenith
        let delay = saastamoinen_trop(lat, h, el);
        // Zenith tropo delay ~2.3–2.5 m at sea level.
        assert!(delay > 2.0 && delay < 3.0, "delay = {}", delay);
    }
}
