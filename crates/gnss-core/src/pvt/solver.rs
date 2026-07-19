//! Weighted Least Squares (WLS) position solver.
//!
//! Iteratively linearizes pseudorange residuals around the current receiver
//! estimate and solves the normal equations. Weights derived from per-channel
//! C/N₀. State: [x, y, z, clk_bias] (4 unknowns).

use crate::nav::ephemeris::{EphemerisMap, IonoParams};
use crate::pvt::atmosphere::{klobuchar_iono, saastamoinen_trop};
use crate::pvt::coords::{ecef_to_lla, lla_to_ecef, Ecef};
use crate::pvt::pseudorange::Pseudorange;
use crate::pvt::satpos::satellite_position;
use nalgebra::{DMatrix, DVector, Matrix3, Matrix4, Vector4};

const C_LIGHT: f64 = 299_792_458.0;
const GPS_L1_CA_CODE_RATE: f64 = 1.023e6;
const GPS_L1_CA_CHIP_M: f64 = C_LIGHT / GPS_L1_CA_CODE_RATE;
const OMEGA_E: f64 = 7.292_115_146_7e-5;
const MAX_ITER: usize = 10;
const CONV_TOL: f64 = 1e-4; // [m]

/// WLS robustness configuration.
#[derive(Debug, Clone)]
pub struct WlsConfig {
    /// Exclude satellites whose broadcast ephemeris health flag is non-zero.
    pub exclude_unhealthy_sv: bool,
    /// Exclude satellites below this elevation once receiver geometry is valid.
    pub elevation_mask_rad: f64,
    /// Post-fit residual threshold for single-satellite RAIM exclusion [m].
    pub raim_residual_threshold_m: f64,
    /// Post-fit residual threshold normalized by the satellite pseudorange sigma.
    pub raim_standardized_residual_threshold: f64,
    /// Maximum number of sequential RAIM exclusions.
    pub raim_max_exclusions: usize,
    /// Reference C/N0 for pseudorange noise calibration [dB-Hz].
    pub cn0_sigma_ref_db_hz: f64,
    /// Receiver/code measurement one-sigma floor at `cn0_sigma_ref_db_hz` [m].
    pub pseudorange_sigma_ref_m: f64,
    /// DLL equivalent noise bandwidth used by the thermal jitter model [Hz].
    pub dll_noise_bandwidth_hz: f64,
    /// DLL coherent integration time used by the thermal jitter model [s].
    pub dll_coherent_integration_s: f64,
    /// Early-minus-late correlator spacing used by the thermal jitter model [chips].
    pub dll_early_late_spacing_chips: f64,
    /// Elevation-dependent residual/multipath one-sigma at zenith [m].
    pub elevation_sigma_zenith_m: f64,
    /// Extra zenith one-sigma for unmodeled L1 ionosphere when Klobuchar is unavailable [m].
    pub iono_sigma_invalid_zenith_m: f64,
    /// Code tracking jitter one-sigma at `cn0_sigma_ref_db_hz` [m].
    pub tracking_jitter_sigma_ref_m: f64,
    /// Lower bound for pseudorange sigma [m].
    pub pseudorange_sigma_min_m: f64,
    /// Upper bound for pseudorange sigma [m].
    pub pseudorange_sigma_max_m: f64,
}

/// Per-satellite diagnostics from the final WLS linearization.
#[derive(Debug, Clone)]
pub struct PvtSatelliteDiagnostic {
    pub prn: u8,
    pub residual_m: f64,
    pub standardized_residual: f64,
    pub azimuth_rad: f64,
    pub elevation_rad: f64,
    pub pseudorange_sigma_m: f64,
    pub cn0_sigma_m: f64,
    pub elevation_sigma_m: f64,
    pub iono_sigma_m: f64,
    pub tracking_jitter_sigma_m: f64,
    pub weight: f64,
    pub iono_delay_m: f64,
    pub tropo_delay_m: f64,
}

impl Default for WlsConfig {
    fn default() -> Self {
        Self {
            exclude_unhealthy_sv: true,
            elevation_mask_rad: 5.0_f64.to_radians(),
            raim_residual_threshold_m: 100.0,
            raim_standardized_residual_threshold: 5.0,
            raim_max_exclusions: 2,
            cn0_sigma_ref_db_hz: 45.0,
            pseudorange_sigma_ref_m: 3.0,
            dll_noise_bandwidth_hz: 2.0,
            dll_coherent_integration_s: 0.02,
            dll_early_late_spacing_chips: 0.5,
            elevation_sigma_zenith_m: 1.0,
            iono_sigma_invalid_zenith_m: 3.0,
            tracking_jitter_sigma_ref_m: 0.3,
            pseudorange_sigma_min_m: 0.5,
            pseudorange_sigma_max_m: 50.0,
        }
    }
}

/// PVT solution (position-only for M2; velocity is a future extension).
#[derive(Debug, Clone)]
pub struct PvtSolution {
    pub ecef: Ecef,
    pub lla: [f64; 3], // [lat_rad, lon_rad, alt_m]
    pub clock_bias_s: f64,
    pub n_sats: usize,
    /// PRNs used in the final solve, aligned with `residuals_m`.
    pub used_prns: Vec<u8>,
    /// PRNs excluded by RAIM.
    pub excluded_prns: Vec<u8>,
    /// Per-satellite post-fit residual [m].
    pub residuals_m: Vec<f64>,
    /// Per-satellite final linearization diagnostics, aligned with `used_prns`.
    pub satellite_diagnostics: Vec<PvtSatelliteDiagnostic>,
    /// Post-fit residual root-mean-square [m].
    pub residual_rms_m: f64,
    /// Post-fit residual RMS normalized by each satellite pseudorange sigma.
    pub standardized_residual_rms: f64,
    /// Largest absolute post-fit residual normalized by satellite pseudorange sigma.
    pub max_standardized_residual: f64,
    /// Conservative epoch quality multiplier for downstream filtering; 1.0 is nominal.
    pub epoch_quality_scale: f64,
    /// WLS receiver position one-sigma estimate from the final normal matrix [m].
    pub position_sigma_m: f64,
    /// Final weighted ECEF position covariance matrix [m^2].
    pub position_covariance_ecef_m2: [[f64; 3]; 3],
    /// East one-sigma estimate from the final weighted normal matrix [m].
    pub east_sigma_m: f64,
    /// North one-sigma estimate from the final weighted normal matrix [m].
    pub north_sigma_m: f64,
    /// Up one-sigma estimate from the final weighted normal matrix [m].
    pub up_sigma_m: f64,
    /// Horizontal one-sigma estimate, sqrt(east^2 + north^2) [m].
    pub horizontal_sigma_m: f64,
    /// Vertical one-sigma estimate, equal to `up_sigma_m` [m].
    pub vertical_sigma_m: f64,
    /// WLS receiver clock-bias one-sigma estimate from the final normal matrix [m].
    pub clock_bias_sigma_m: f64,
    /// Horizontal dilution of precision from the final unweighted geometry.
    pub hdop: f64,
    /// Vertical dilution of precision from the final unweighted geometry.
    pub vdop: f64,
    /// Position dilution of precision from the final unweighted geometry.
    pub pdop: f64,
    /// Geometric dilution of precision from the final unweighted geometry.
    pub gdop: f64,
    pub converged: bool,
}

impl Default for PvtSolution {
    fn default() -> Self {
        Self {
            ecef: [0.0; 3],
            lla: [0.0; 3],
            clock_bias_s: 0.0,
            n_sats: 0,
            used_prns: Vec::new(),
            excluded_prns: Vec::new(),
            residuals_m: Vec::new(),
            satellite_diagnostics: Vec::new(),
            residual_rms_m: 0.0,
            standardized_residual_rms: 0.0,
            max_standardized_residual: 0.0,
            epoch_quality_scale: 1.0,
            position_sigma_m: 0.0,
            position_covariance_ecef_m2: [[0.0; 3]; 3],
            east_sigma_m: 0.0,
            north_sigma_m: 0.0,
            up_sigma_m: 0.0,
            horizontal_sigma_m: 0.0,
            vertical_sigma_m: 0.0,
            clock_bias_sigma_m: 0.0,
            hdop: 0.0,
            vdop: 0.0,
            pdop: 0.0,
            gdop: 0.0,
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
    solve_wls_with_config(
        pseudoranges,
        ephemerides,
        iono,
        rx_lat_rad,
        rx_lon_rad,
        rx_alt_m,
        &WlsConfig::default(),
    )
}

pub fn solve_wls_with_config(
    pseudoranges: &[Pseudorange],
    ephemerides: &EphemerisMap,
    iono: &IonoParams,
    rx_lat_rad: f64,
    rx_lon_rad: f64,
    rx_alt_m: f64,
    config: &WlsConfig,
) -> PvtSolution {
    let mut excluded_prns = Vec::new();
    let mut solution = solve_wls_once(
        pseudoranges,
        ephemerides,
        iono,
        rx_lat_rad,
        rx_lon_rad,
        rx_alt_m,
        config,
        &excluded_prns,
    );

    for _ in 0..config.raim_max_exclusions {
        let Some(prn) = raim_exclusion_candidate(&solution, config) else {
            break;
        };
        excluded_prns.push(prn);
        solution = solve_wls_once(
            pseudoranges,
            ephemerides,
            iono,
            rx_lat_rad,
            rx_lon_rad,
            rx_alt_m,
            config,
            &excluded_prns,
        );
    }

    solution.excluded_prns = excluded_prns;
    solution
}

#[allow(clippy::too_many_arguments)]
fn solve_wls_once(
    pseudoranges: &[Pseudorange],
    ephemerides: &EphemerisMap,
    iono: &IonoParams,
    rx_lat_rad: f64,
    rx_lon_rad: f64,
    rx_alt_m: f64,
    config: &WlsConfig,
    excluded_prns: &[u8],
) -> PvtSolution {
    let n = pseudoranges.len();
    if n < 4 {
        return PvtSolution {
            n_sats: n,
            ..Default::default()
        };
    }
    let has_apriori =
        rx_lat_rad.abs() > 1e-12 || rx_lon_rad.abs() > 1e-12 || rx_alt_m.abs() > 1e-12;

    // Initial estimate: use caller-provided approximate coordinates when
    // available; otherwise start from Earth center with zero clock bias.
    let mut x = Vector4::<f64>::zeros(); // [x, y, z, clk_bias_m]
    if has_apriori {
        let apriori = lla_to_ecef([rx_lat_rad, rx_lon_rad, rx_alt_m]);
        x[0] = apriori[0];
        x[1] = apriori[1];
        x[2] = apriori[2];
    }

    let mut last_residuals: Vec<f64> = Vec::new();
    let mut last_used_prns: Vec<u8> = Vec::new();
    let mut last_satellite_diagnostics: Vec<PvtSatelliteDiagnostic> = Vec::new();
    let mut last_normal_inverse: Option<Matrix4<f64>> = None;
    let mut last_geometry_inverse: Option<Matrix4<f64>> = None;
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
        let mut used_prns = Vec::with_capacity(pseudoranges.len());
        let mut satellite_diagnostics = Vec::with_capacity(pseudoranges.len());

        for pr in pseudoranges {
            if excluded_prns.contains(&pr.prn) {
                continue;
            }
            let eph = match ephemerides.get(&pr.prn) {
                Some(e) => e,
                None => continue,
            };
            if config.exclude_unhealthy_sv && eph.sv_health != 0 {
                continue;
            }
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
            if range < 1.0 {
                continue;
            }
            let ex = dx / range;
            let ey = dy / range;
            let ez = dz / range;

            // Azimuth / elevation for atmospheric corrections
            let (az, el) = azel(rx_pos, [xs_r, ys_r, zs_r]);

            // Atmospheric delays require a meaningful receiver position for
            // azimuth/elevation. If bootstrapping from Earth center, skip them
            // on the first linearization and enable after the first update.
            let apply_atmosphere = has_apriori || iter > 0;
            if apply_atmosphere && el < config.elevation_mask_rad {
                continue;
            }
            let iono_m = if apply_atmosphere && iono.valid {
                klobuchar_iono(iono, lat_r, lon_r, az, el, t_sv)
            } else {
                0.0
            };
            let trop_m = if apply_atmosphere {
                saastamoinen_trop(lat_r, alt_r, el)
            } else {
                0.0
            };

            // Corrected pseudorange (remove sat clock, add atmosphere)
            let pr_corrected =
                pr.pseudorange_m + C_LIGHT * sat.clock_correction_s - iono_m - trop_m;

            // Residual: measured - predicted
            let predicted = range + x[3]; // + receiver clock bias (in meters)
            let resid = pr_corrected - predicted;

            // Weight from C/N0: higher C/N0 → smaller variance.
            let sigma_model = pseudorange_sigma_model(
                pr.cn0_db_hz,
                pr.tracking_jitter_chips,
                apply_atmosphere.then_some(el),
                iono.valid,
                config,
            );
            let weight = 1.0 / (sigma_model.total_m * sigma_model.total_m);
            let standardized_residual = resid / sigma_model.total_m.max(1e-6);

            v[used] = resid;
            h[(used, 0)] = -ex;
            h[(used, 1)] = -ey;
            h[(used, 2)] = -ez;
            h[(used, 3)] = 1.0;
            w_diag[used] = weight;
            used_prns.push(pr.prn);
            satellite_diagnostics.push(PvtSatelliteDiagnostic {
                prn: pr.prn,
                residual_m: resid,
                standardized_residual,
                azimuth_rad: az,
                elevation_rad: el,
                pseudorange_sigma_m: sigma_model.total_m,
                cn0_sigma_m: sigma_model.cn0_m,
                elevation_sigma_m: sigma_model.elevation_m,
                iono_sigma_m: sigma_model.iono_m,
                tracking_jitter_sigma_m: sigma_model.tracking_jitter_m,
                weight,
                iono_delay_m: iono_m,
                tropo_delay_m: trop_m,
            });
            used += 1;
        }

        if used < 4 {
            return PvtSolution {
                n_sats: used,
                ..Default::default()
            };
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
        let hth4: Matrix4<f64> = (&h.transpose() * &h).fixed_resize::<4, 4>(0.0);

        let normal_inverse = match htw4.try_inverse() {
            Some(inv) => inv,
            None => {
                return PvtSolution {
                    n_sats: used,
                    ..Default::default()
                }
            }
        };
        let geometry_inverse = hth4.try_inverse();
        let dx = normal_inverse * htwv4;

        x[0] += dx[0];
        x[1] += dx[1];
        x[2] += dx[2];
        x[3] += dx[3];

        last_residuals = (0..used).map(|i| v[i]).collect();
        last_used_prns = used_prns;
        last_satellite_diagnostics = satellite_diagnostics;
        last_normal_inverse = Some(normal_inverse);
        last_geometry_inverse = geometry_inverse;
        used_count = used;

        let step = dx.rows(0, 3).norm();
        if step < CONV_TOL {
            converged = true;
            break;
        }
    }

    let ecef: Ecef = [x[0], x[1], x[2]];
    let lla = ecef_to_lla(ecef);
    let uncertainty = solution_uncertainty(
        last_normal_inverse.as_ref(),
        last_geometry_inverse.as_ref(),
        lla,
    );
    let quality = solution_quality(
        &last_residuals,
        &last_satellite_diagnostics,
        uncertainty.gdop,
    );

    PvtSolution {
        ecef,
        lla,
        clock_bias_s: x[3] / C_LIGHT,
        n_sats: used_count,
        used_prns: last_used_prns,
        excluded_prns: excluded_prns.to_vec(),
        residuals_m: last_residuals,
        satellite_diagnostics: last_satellite_diagnostics,
        residual_rms_m: quality.residual_rms_m,
        standardized_residual_rms: quality.standardized_residual_rms,
        max_standardized_residual: quality.max_standardized_residual,
        epoch_quality_scale: quality.epoch_quality_scale,
        position_sigma_m: uncertainty.position_sigma_m,
        position_covariance_ecef_m2: uncertainty.position_covariance_ecef_m2,
        east_sigma_m: uncertainty.east_sigma_m,
        north_sigma_m: uncertainty.north_sigma_m,
        up_sigma_m: uncertainty.up_sigma_m,
        horizontal_sigma_m: uncertainty.horizontal_sigma_m,
        vertical_sigma_m: uncertainty.vertical_sigma_m,
        clock_bias_sigma_m: uncertainty.clock_bias_sigma_m,
        hdop: uncertainty.hdop,
        vdop: uncertainty.vdop,
        pdop: uncertainty.pdop,
        gdop: uncertainty.gdop,
        converged,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SolutionUncertainty {
    position_sigma_m: f64,
    position_covariance_ecef_m2: [[f64; 3]; 3],
    east_sigma_m: f64,
    north_sigma_m: f64,
    up_sigma_m: f64,
    horizontal_sigma_m: f64,
    vertical_sigma_m: f64,
    clock_bias_sigma_m: f64,
    hdop: f64,
    vdop: f64,
    pdop: f64,
    gdop: f64,
}

#[derive(Debug, Clone, Copy)]
struct SolutionQuality {
    residual_rms_m: f64,
    standardized_residual_rms: f64,
    max_standardized_residual: f64,
    epoch_quality_scale: f64,
}

impl Default for SolutionQuality {
    fn default() -> Self {
        Self {
            residual_rms_m: 0.0,
            standardized_residual_rms: 0.0,
            max_standardized_residual: 0.0,
            epoch_quality_scale: 1.0,
        }
    }
}

fn solution_quality(
    residuals_m: &[f64],
    diagnostics: &[PvtSatelliteDiagnostic],
    gdop: f64,
) -> SolutionQuality {
    let residual_rms_m = rms(residuals_m).unwrap_or(0.0);
    let standardized: Vec<f64> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.standardized_residual)
        .filter(|value| value.is_finite())
        .collect();
    let standardized_residual_rms = rms(&standardized).unwrap_or(0.0);
    let max_standardized_residual = standardized
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f64, f64::max);

    let standardized_rms_scale = if standardized_residual_rms > 1.5 {
        standardized_residual_rms / 1.5
    } else {
        1.0
    };
    let max_residual_scale = if max_standardized_residual > 3.0 {
        max_standardized_residual / 3.0
    } else {
        1.0
    };
    let geometry_scale = if gdop.is_finite() && gdop > 6.0 {
        gdop / 6.0
    } else {
        1.0
    };

    SolutionQuality {
        residual_rms_m,
        standardized_residual_rms,
        max_standardized_residual,
        epoch_quality_scale: standardized_rms_scale
            .max(max_residual_scale)
            .max(geometry_scale)
            .clamp(1.0, 25.0),
    }
}

fn solution_uncertainty(
    weighted_inverse: Option<&Matrix4<f64>>,
    geometry_inverse: Option<&Matrix4<f64>>,
    lla: [f64; 3],
) -> SolutionUncertainty {
    let mut uncertainty = SolutionUncertainty::default();
    if let Some(weighted_inverse) = weighted_inverse {
        uncertainty.position_covariance_ecef_m2 = ecef_position_covariance_array(weighted_inverse);
        let enu_cov = ecef_position_covariance_to_enu(weighted_inverse, lla);
        uncertainty.east_sigma_m = enu_cov[(0, 0)].max(0.0).sqrt();
        uncertainty.north_sigma_m = enu_cov[(1, 1)].max(0.0).sqrt();
        uncertainty.up_sigma_m = enu_cov[(2, 2)].max(0.0).sqrt();
        uncertainty.horizontal_sigma_m =
            (uncertainty.east_sigma_m.powi(2) + uncertainty.north_sigma_m.powi(2)).sqrt();
        uncertainty.vertical_sigma_m = uncertainty.up_sigma_m;
        uncertainty.position_sigma_m = ((uncertainty.east_sigma_m.powi(2)
            + uncertainty.north_sigma_m.powi(2)
            + uncertainty.up_sigma_m.powi(2))
            / 3.0)
            .sqrt();
        uncertainty.clock_bias_sigma_m = weighted_inverse[(3, 3)].max(0.0).sqrt();
    }
    if let Some(geometry_inverse) = geometry_inverse {
        let enu_dop = ecef_position_covariance_to_enu(geometry_inverse, lla);
        let east_dop2 = enu_dop[(0, 0)].max(0.0);
        let north_dop2 = enu_dop[(1, 1)].max(0.0);
        let up_dop2 = enu_dop[(2, 2)].max(0.0);
        let clock_dop2 = geometry_inverse[(3, 3)].max(0.0);
        uncertainty.hdop = (east_dop2 + north_dop2).sqrt();
        uncertainty.vdop = up_dop2.sqrt();
        uncertainty.pdop = (east_dop2 + north_dop2 + up_dop2).sqrt();
        uncertainty.gdop = (east_dop2 + north_dop2 + up_dop2 + clock_dop2).sqrt();
    }
    uncertainty
}

fn ecef_position_covariance_array(covariance4: &Matrix4<f64>) -> [[f64; 3]; 3] {
    [
        [
            covariance4[(0, 0)],
            covariance4[(0, 1)],
            covariance4[(0, 2)],
        ],
        [
            covariance4[(1, 0)],
            covariance4[(1, 1)],
            covariance4[(1, 2)],
        ],
        [
            covariance4[(2, 0)],
            covariance4[(2, 1)],
            covariance4[(2, 2)],
        ],
    ]
}

fn rms(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some((values.iter().map(|value| value * value).sum::<f64>() / values.len() as f64).sqrt())
}

fn ecef_position_covariance_to_enu(covariance4: &Matrix4<f64>, lla: [f64; 3]) -> Matrix3<f64> {
    let covariance = Matrix3::new(
        covariance4[(0, 0)],
        covariance4[(0, 1)],
        covariance4[(0, 2)],
        covariance4[(1, 0)],
        covariance4[(1, 1)],
        covariance4[(1, 2)],
        covariance4[(2, 0)],
        covariance4[(2, 1)],
        covariance4[(2, 2)],
    );
    let rotation = ecef_to_enu_rotation(lla[0], lla[1]);
    rotation * covariance * rotation.transpose()
}

fn ecef_to_enu_rotation(lat_rad: f64, lon_rad: f64) -> Matrix3<f64> {
    let sin_lat = lat_rad.sin();
    let cos_lat = lat_rad.cos();
    let sin_lon = lon_rad.sin();
    let cos_lon = lon_rad.cos();
    Matrix3::new(
        -sin_lon,
        cos_lon,
        0.0,
        -sin_lat * cos_lon,
        -sin_lat * sin_lon,
        cos_lat,
        cos_lat * cos_lon,
        cos_lat * sin_lon,
        sin_lat,
    )
}

fn raim_exclusion_candidate(solution: &PvtSolution, config: &WlsConfig) -> Option<u8> {
    if solution.used_prns.len() <= 4
        || solution.used_prns.len() != solution.residuals_m.len()
        || solution.used_prns.len() != solution.satellite_diagnostics.len()
    {
        return None;
    }

    solution
        .satellite_diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.residual_m.abs() > config.raim_residual_threshold_m
                || diagnostic.standardized_residual.abs()
                    > config.raim_standardized_residual_threshold
        })
        .max_by(|a, b| {
            a.standardized_residual
                .abs()
                .total_cmp(&b.standardized_residual.abs())
                .then_with(|| a.residual_m.abs().total_cmp(&b.residual_m.abs()))
        })
        .map(|diagnostic| diagnostic.prn)
}

#[derive(Debug, Clone, Copy)]
struct PseudorangeSigmaModel {
    total_m: f64,
    cn0_m: f64,
    elevation_m: f64,
    iono_m: f64,
    tracking_jitter_m: f64,
}

fn pseudorange_sigma_model(
    cn0_db_hz: f64,
    tracking_jitter_chips: f64,
    elevation_rad: Option<f64>,
    iono_valid: bool,
    config: &WlsConfig,
) -> PseudorangeSigmaModel {
    let cn0_m = dll_thermal_jitter_sigma_m(cn0_db_hz, config);
    let cn0_ratio = cn0_linear_sigma_ratio(cn0_db_hz, config);
    let code_floor_m = pseudorange_sigma_from_cn0(cn0_db_hz, config);
    let elevation_m = elevation_rad
        .map(|el| config.elevation_sigma_zenith_m / el.sin().clamp(0.1, 1.0))
        .unwrap_or(0.0);
    let iono_m = match (elevation_rad, iono_valid) {
        (Some(el), false) => config.iono_sigma_invalid_zenith_m / el.sin().clamp(0.1, 1.0),
        _ => 0.0,
    };
    let inferred_tracking_jitter_m = config.tracking_jitter_sigma_ref_m * cn0_ratio
        / elevation_rad
            .map(|el| el.sin().clamp(0.25, 1.0))
            .unwrap_or(1.0);
    let observed_tracking_jitter_m = if tracking_jitter_chips.is_finite() {
        tracking_jitter_chips.abs() * GPS_L1_CA_CHIP_M
    } else {
        0.0
    };
    let tracking_jitter_m = inferred_tracking_jitter_m.max(observed_tracking_jitter_m);
    let total_m = (code_floor_m * code_floor_m
        + cn0_m * cn0_m
        + elevation_m * elevation_m
        + iono_m * iono_m
        + tracking_jitter_m * tracking_jitter_m)
        .sqrt()
        .clamp(
            config.pseudorange_sigma_min_m,
            config.pseudorange_sigma_max_m,
        );
    PseudorangeSigmaModel {
        total_m,
        cn0_m,
        elevation_m,
        iono_m,
        tracking_jitter_m,
    }
}

fn pseudorange_sigma_from_cn0(cn0_db_hz: f64, config: &WlsConfig) -> f64 {
    let sigma = config.pseudorange_sigma_ref_m * cn0_linear_sigma_ratio(cn0_db_hz, config);
    sigma.clamp(
        config.pseudorange_sigma_min_m,
        config.pseudorange_sigma_max_m,
    )
}

fn dll_thermal_jitter_sigma_m(cn0_db_hz: f64, config: &WlsConfig) -> f64 {
    let cn0_hz = 10.0_f64.powf(cn0_db_hz / 10.0);
    let bandwidth_hz = config.dll_noise_bandwidth_hz.max(1e-6);
    let integration_s = config.dll_coherent_integration_s.max(1e-6);
    let spacing_chips = config.dll_early_late_spacing_chips.clamp(0.05, 2.0);
    let discriminator_slope = (2.0 * spacing_chips).max(1e-6);
    let jitter_chips = (bandwidth_hz / cn0_hz * (1.0 + 2.0 / (integration_s * cn0_hz))).sqrt()
        / discriminator_slope;
    (jitter_chips * GPS_L1_CA_CHIP_M).clamp(
        config.pseudorange_sigma_min_m,
        config.pseudorange_sigma_max_m,
    )
}

fn cn0_linear_sigma_ratio(cn0_db_hz: f64, config: &WlsConfig) -> f64 {
    let ratio = 10.0_f64.powf((config.cn0_sigma_ref_db_hz - cn0_db_hz) / 10.0);
    ratio.sqrt()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wls_config_defaults_enable_mask_and_raim() {
        let cfg = WlsConfig::default();
        assert!(cfg.exclude_unhealthy_sv);
        assert!((cfg.elevation_mask_rad.to_degrees() - 5.0).abs() < 1e-12);
        assert_eq!(cfg.raim_residual_threshold_m, 100.0);
        assert_eq!(cfg.raim_standardized_residual_threshold, 5.0);
        assert_eq!(cfg.raim_max_exclusions, 2);
        assert_eq!(cfg.cn0_sigma_ref_db_hz, 45.0);
        assert_eq!(cfg.pseudorange_sigma_ref_m, 3.0);
        assert_eq!(cfg.dll_noise_bandwidth_hz, 2.0);
        assert_eq!(cfg.dll_coherent_integration_s, 0.02);
        assert_eq!(cfg.dll_early_late_spacing_chips, 0.5);
        assert_eq!(cfg.elevation_sigma_zenith_m, 1.0);
        assert_eq!(cfg.iono_sigma_invalid_zenith_m, 3.0);
        assert_eq!(cfg.tracking_jitter_sigma_ref_m, 0.3);
        assert_eq!(cfg.pseudorange_sigma_min_m, 0.5);
        assert_eq!(cfg.pseudorange_sigma_max_m, 50.0);
    }

    #[test]
    fn test_wls_excludes_unhealthy_satellites_by_default() {
        let rx = lla_to_ecef([41.0_f64.to_radians(), 2.0_f64.to_radians(), 100.0]);
        let mut ephemerides = synthetic_ephemerides(5);
        ephemerides.get_mut(&3).unwrap().sv_health = 1;
        let pseudoranges = synthetic_pseudoranges(&ephemerides, rx, 42.0);
        let config = WlsConfig {
            elevation_mask_rad: -90.0_f64.to_radians(),
            raim_max_exclusions: 0,
            ..Default::default()
        };

        let solution = solve_wls_with_config(
            &pseudoranges,
            &ephemerides,
            &IonoParams::default(),
            41.0_f64.to_radians(),
            2.0_f64.to_radians(),
            100.0,
            &config,
        );

        assert_eq!(solution.n_sats, 4);
        assert!(!solution.used_prns.contains(&3));
    }

    #[test]
    fn test_wls_can_use_more_than_twelve_satellites() {
        let rx = lla_to_ecef([41.0_f64.to_radians(), 2.0_f64.to_radians(), 100.0]);
        let ephemerides = synthetic_ephemerides(13);
        let pseudoranges = synthetic_pseudoranges(&ephemerides, rx, 42.0);
        let config = WlsConfig {
            elevation_mask_rad: -90.0_f64.to_radians(),
            raim_max_exclusions: 0,
            exclude_unhealthy_sv: true,
            ..Default::default()
        };

        let solution = solve_wls_with_config(
            &pseudoranges,
            &ephemerides,
            &IonoParams::default(),
            41.0_f64.to_radians(),
            2.0_f64.to_radians(),
            100.0,
            &config,
        );

        assert_eq!(solution.n_sats, 13);
        assert_eq!(solution.used_prns.len(), 13);
    }

    #[test]
    fn test_pseudorange_sigma_from_cn0_is_bounded_and_monotonic() {
        let cfg = WlsConfig::default();

        let sigma_35 = pseudorange_sigma_from_cn0(35.0, &cfg);
        let sigma_45 = pseudorange_sigma_from_cn0(45.0, &cfg);
        let sigma_55 = pseudorange_sigma_from_cn0(55.0, &cfg);

        assert!((sigma_45 - cfg.pseudorange_sigma_ref_m).abs() < 1e-12);
        assert!(sigma_35 > sigma_45);
        assert!(sigma_55 < sigma_45);
        assert_eq!(
            pseudorange_sigma_from_cn0(-10.0, &cfg),
            cfg.pseudorange_sigma_max_m
        );
        assert_eq!(
            pseudorange_sigma_from_cn0(90.0, &cfg),
            cfg.pseudorange_sigma_min_m
        );
    }

    #[test]
    fn test_dll_thermal_jitter_uses_bandwidth_cn0_and_integration_time() {
        let cfg = WlsConfig {
            pseudorange_sigma_min_m: 0.0,
            pseudorange_sigma_max_m: 1_000.0,
            ..Default::default()
        };

        let nominal = dll_thermal_jitter_sigma_m(45.0, &cfg);
        let weak_signal = dll_thermal_jitter_sigma_m(35.0, &cfg);
        let wide_bandwidth = dll_thermal_jitter_sigma_m(
            45.0,
            &WlsConfig {
                dll_noise_bandwidth_hz: cfg.dll_noise_bandwidth_hz * 4.0,
                pseudorange_sigma_min_m: 0.0,
                pseudorange_sigma_max_m: 1_000.0,
                ..cfg.clone()
            },
        );
        let long_integration = dll_thermal_jitter_sigma_m(
            45.0,
            &WlsConfig {
                dll_coherent_integration_s: cfg.dll_coherent_integration_s * 4.0,
                pseudorange_sigma_min_m: 0.0,
                pseudorange_sigma_max_m: 1_000.0,
                ..cfg.clone()
            },
        );

        assert!(nominal > 0.0);
        assert!(weak_signal > nominal);
        assert!(wide_bandwidth > nominal);
        assert!(long_integration < nominal);
    }

    #[test]
    fn test_pseudorange_sigma_model_combines_cn0_elevation_and_tracking_jitter() {
        let cfg = WlsConfig::default();

        let high_quality =
            pseudorange_sigma_model(50.0, 0.0, Some(80.0_f64.to_radians()), true, &cfg);
        let low_cn0 = pseudorange_sigma_model(35.0, 0.0, Some(80.0_f64.to_radians()), true, &cfg);
        let low_elevation =
            pseudorange_sigma_model(50.0, 0.0, Some(10.0_f64.to_radians()), true, &cfg);
        let high_dll_jitter =
            pseudorange_sigma_model(50.0, 0.05, Some(80.0_f64.to_radians()), true, &cfg);
        let invalid_iono =
            pseudorange_sigma_model(50.0, 0.0, Some(80.0_f64.to_radians()), false, &cfg);

        assert!(high_quality.total_m > high_quality.cn0_m);
        assert!(high_quality.tracking_jitter_m > 0.0);
        assert_eq!(high_quality.iono_m, 0.0);
        assert!(low_cn0.total_m > high_quality.total_m);
        assert!(low_elevation.elevation_m > high_quality.elevation_m);
        assert!(low_elevation.total_m > high_quality.total_m);
        assert!(high_dll_jitter.tracking_jitter_m > high_quality.tracking_jitter_m);
        assert!(high_dll_jitter.total_m > high_quality.total_m);
        assert!(invalid_iono.iono_m > 0.0);
        assert!(invalid_iono.total_m > high_quality.total_m);
    }

    #[test]
    fn test_solution_uncertainty_derives_enu_sigmas_and_dops() {
        let mut weighted_inverse = Matrix4::<f64>::zeros();
        weighted_inverse[(0, 0)] = 9.0;
        weighted_inverse[(1, 1)] = 16.0;
        weighted_inverse[(2, 2)] = 25.0;
        weighted_inverse[(3, 3)] = 100.0;
        let geometry_inverse = Matrix4::<f64>::identity();

        let uncertainty = solution_uncertainty(
            Some(&weighted_inverse),
            Some(&geometry_inverse),
            [0.0, 0.0, 0.0],
        );

        assert_eq!(uncertainty.east_sigma_m, 4.0);
        assert_eq!(uncertainty.north_sigma_m, 5.0);
        assert_eq!(uncertainty.up_sigma_m, 3.0);
        assert_eq!(
            uncertainty.position_covariance_ecef_m2,
            [[9.0, 0.0, 0.0], [0.0, 16.0, 0.0], [0.0, 0.0, 25.0]]
        );
        assert!((uncertainty.horizontal_sigma_m - (41.0_f64).sqrt()).abs() < 1e-12);
        assert_eq!(uncertainty.vertical_sigma_m, 3.0);
        assert!((uncertainty.position_sigma_m - (50.0_f64 / 3.0).sqrt()).abs() < 1e-12);
        assert_eq!(uncertainty.clock_bias_sigma_m, 10.0);
        assert!((uncertainty.hdop - 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(uncertainty.vdop, 1.0);
        assert!((uncertainty.pdop - 3.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(uncertainty.gdop, 2.0);
    }

    #[test]
    fn test_solution_quality_scales_bad_residuals_and_geometry() {
        let nominal = solution_quality(
            &[1.0, -2.0, 1.5, -1.0],
            &[
                diagnostic(1, 1.0, 3.0),
                diagnostic(2, -2.0, 3.0),
                diagnostic(3, 1.5, 3.0),
                diagnostic(4, -1.0, 3.0),
            ],
            2.5,
        );
        assert!(nominal.residual_rms_m > 0.0);
        assert!(nominal.standardized_residual_rms < 1.0);
        assert_eq!(nominal.epoch_quality_scale, 1.0);

        let residual_outlier = solution_quality(
            &[1.0, -2.0, 12.0, -1.0],
            &[
                diagnostic(1, 1.0, 3.0),
                diagnostic(2, -2.0, 3.0),
                diagnostic(3, 12.0, 3.0),
                diagnostic(4, -1.0, 3.0),
            ],
            2.5,
        );
        assert_eq!(residual_outlier.max_standardized_residual, 4.0);
        assert!(residual_outlier.epoch_quality_scale > 1.0);

        let weak_geometry = solution_quality(&[0.0; 4], &[], 12.0);
        assert_eq!(weak_geometry.epoch_quality_scale, 2.0);
    }

    #[test]
    fn test_raim_exclusion_candidate_requires_redundancy_and_threshold() {
        let cfg = WlsConfig::default();

        let absolute_outlier = raim_solution(&[
            (1, 0.5, 3.0),
            (2, -2.0, 3.0),
            (3, 125.0, 50.0),
            (4, 3.0, 3.0),
            (5, -4.0, 3.0),
        ]);
        assert_eq!(raim_exclusion_candidate(&absolute_outlier, &cfg), Some(3));

        let standardized_outlier = raim_solution(&[
            (1, 0.5, 3.0),
            (2, -2.0, 3.0),
            (3, 20.0, 2.0),
            (4, 3.0, 3.0),
            (5, -4.0, 3.0),
        ]);
        assert_eq!(
            raim_exclusion_candidate(&standardized_outlier, &cfg),
            Some(3)
        );

        let high_sigma_residual = raim_solution(&[
            (1, 0.5, 3.0),
            (2, -2.0, 3.0),
            (3, 20.0, 20.0),
            (4, 3.0, 3.0),
            (5, -4.0, 3.0),
        ]);
        assert_eq!(raim_exclusion_candidate(&high_sigma_residual, &cfg), None);

        let four_satellite_outlier = raim_solution(&[
            (1, 0.5, 3.0),
            (2, -2.0, 3.0),
            (3, 125.0, 50.0),
            (4, 3.0, 3.0),
        ]);
        assert_eq!(
            raim_exclusion_candidate(&four_satellite_outlier, &cfg),
            None
        );
    }

    fn raim_solution(entries: &[(u8, f64, f64)]) -> PvtSolution {
        PvtSolution {
            used_prns: entries.iter().map(|(prn, _, _)| *prn).collect(),
            residuals_m: entries
                .iter()
                .map(|(_, residual_m, _)| *residual_m)
                .collect(),
            satellite_diagnostics: entries
                .iter()
                .map(|(prn, residual_m, sigma_m)| diagnostic(*prn, *residual_m, *sigma_m))
                .collect(),
            ..Default::default()
        }
    }

    fn diagnostic(prn: u8, residual_m: f64, pseudorange_sigma_m: f64) -> PvtSatelliteDiagnostic {
        PvtSatelliteDiagnostic {
            prn,
            residual_m,
            standardized_residual: residual_m / pseudorange_sigma_m,
            azimuth_rad: 0.0,
            elevation_rad: 0.5,
            pseudorange_sigma_m,
            cn0_sigma_m: pseudorange_sigma_m,
            elevation_sigma_m: 0.0,
            iono_sigma_m: 0.0,
            tracking_jitter_sigma_m: 0.0,
            weight: 1.0 / (pseudorange_sigma_m * pseudorange_sigma_m),
            iono_delay_m: 0.0,
            tropo_delay_m: 0.0,
        }
    }

    fn synthetic_ephemerides(count: u8) -> EphemerisMap {
        (1..=count)
            .map(|prn| {
                let phase = prn as f64 * 0.47;
                (
                    prn,
                    crate::nav::ephemeris::Ephemeris {
                        prn,
                        sqrt_a: 26_559_710.0_f64.sqrt(),
                        eccentricity: 0.01,
                        m0: phase,
                        omega: phase * 0.5,
                        omega0: phase * 0.8,
                        omega_dot: -8.0e-9,
                        i0: 55.0_f64.to_radians(),
                        i_dot: 0.0,
                        delta_n: 0.0,
                        toe: 0.0,
                        toc: 0.0,
                        ..Default::default()
                    },
                )
            })
            .collect()
    }

    fn synthetic_pseudoranges(
        ephemerides: &EphemerisMap,
        receiver_ecef: Ecef,
        receiver_clock_bias_m: f64,
    ) -> Vec<Pseudorange> {
        ephemerides
            .iter()
            .map(|(prn, eph)| {
                let sat = satellite_position(eph, 100.0);
                let dx = sat.ecef[0] - receiver_ecef[0];
                let dy = sat.ecef[1] - receiver_ecef[1];
                let dz = sat.ecef[2] - receiver_ecef[2];
                Pseudorange {
                    prn: *prn,
                    pseudorange_m: (dx * dx + dy * dy + dz * dz).sqrt() + receiver_clock_bias_m,
                    transmit_time_sow: 100.0,
                    receive_time_sow: 100.075,
                    cn0_db_hz: 45.0,
                    doppler_hz: 0.0,
                    tracking_jitter_chips: 0.0,
                }
            })
            .collect()
    }

    #[test]
    fn test_satellite_diagnostics_are_aligned_with_solution_vectors() {
        let solution = PvtSolution {
            used_prns: vec![3, 7],
            residuals_m: vec![1.25, -0.75],
            satellite_diagnostics: vec![
                PvtSatelliteDiagnostic {
                    prn: 3,
                    residual_m: 1.25,
                    standardized_residual: 1.25 / 3.0,
                    azimuth_rad: 0.1,
                    elevation_rad: 0.5,
                    pseudorange_sigma_m: 3.0,
                    cn0_sigma_m: 2.8,
                    elevation_sigma_m: 0.9,
                    iono_sigma_m: 0.0,
                    tracking_jitter_sigma_m: 0.2,
                    weight: 1.0 / 9.0,
                    iono_delay_m: 0.0,
                    tropo_delay_m: 2.0,
                },
                PvtSatelliteDiagnostic {
                    prn: 7,
                    residual_m: -0.75,
                    standardized_residual: -0.75 / 2.0,
                    azimuth_rad: 0.2,
                    elevation_rad: 0.6,
                    pseudorange_sigma_m: 2.0,
                    cn0_sigma_m: 1.8,
                    elevation_sigma_m: 0.8,
                    iono_sigma_m: 0.0,
                    tracking_jitter_sigma_m: 0.1,
                    weight: 0.25,
                    iono_delay_m: 0.0,
                    tropo_delay_m: 2.2,
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            solution.satellite_diagnostics.len(),
            solution.used_prns.len()
        );
        for (idx, diagnostic) in solution.satellite_diagnostics.iter().enumerate() {
            assert_eq!(diagnostic.prn, solution.used_prns[idx]);
            assert_eq!(diagnostic.residual_m, solution.residuals_m[idx]);
            assert!(diagnostic.elevation_rad.is_finite());
            assert!(diagnostic.pseudorange_sigma_m > 0.0);
            assert!(diagnostic.weight > 0.0);
        }
    }
}
