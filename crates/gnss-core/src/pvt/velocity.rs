//! Doppler-based receiver velocity solution.
//!
//! The Doppler sign convention follows the tracking and Hatch smoothing path:
//! observed pseudorange rate is `rho_dot = -lambda * doppler_hz`.

use crate::nav::ephemeris::{Ephemeris, EphemerisMap};
use crate::pvt::coords::{ecef_to_lla, Ecef};
use crate::pvt::pseudorange::Pseudorange;
use crate::pvt::satpos::{satellite_clock_drift, satellite_position};
use nalgebra::{DMatrix, DVector, Matrix4, Vector4};

const C_LIGHT: f64 = 299_792_458.0;
const GPS_L1_HZ: f64 = 1_575_420_000.0;
const GPS_L1_WAVELENGTH_M: f64 = C_LIGHT / GPS_L1_HZ;
const OMEGA_E: f64 = 7.292_115_146_7e-5;

#[derive(Debug, Clone, Copy)]
pub struct DopplerVelocityObservation {
    pub prn: u8,
    /// Unit vector from receiver to satellite in ECEF.
    pub line_of_sight: Ecef,
    /// Satellite velocity in the same ECEF frame as `line_of_sight` [m/s].
    pub satellite_velocity_mps: Ecef,
    /// Satellite position at the transmit epoch [m].
    pub satellite_ecef: Ecef,
    /// Satellite clock drift at the transmit epoch [s/s].
    pub satellite_clock_drift_sps: f64,
    /// Carrier Doppler from tracking [Hz].
    pub doppler_hz: f64,
    /// Carrier-to-noise density [dB-Hz], retained for quality diagnostics.
    pub cn0_db_hz: f64,
}

#[derive(Debug, Clone)]
pub struct VelocitySolution {
    pub ecef_velocity_mps: Ecef,
    /// East, north, up receiver velocity [m/s].
    pub enu_velocity_mps: Ecef,
    pub clock_drift_mps: f64,
    pub clock_drift_sps: f64,
    pub used_prns: Vec<u8>,
    pub residuals_mps: Vec<f64>,
    pub converged: bool,
}

impl Default for VelocitySolution {
    fn default() -> Self {
        Self {
            ecef_velocity_mps: [0.0; 3],
            enu_velocity_mps: [0.0; 3],
            clock_drift_mps: 0.0,
            clock_drift_sps: 0.0,
            used_prns: Vec::new(),
            residuals_mps: Vec::new(),
            converged: false,
        }
    }
}

pub fn solve_doppler_velocity(
    pseudoranges: &[Pseudorange],
    ephemerides: &EphemerisMap,
    receiver_ecef: Ecef,
) -> VelocitySolution {
    let observations = doppler_velocity_observations(pseudoranges, ephemerides, receiver_ecef);
    solve_doppler_velocity_from_observations(&observations, receiver_ecef)
}

pub fn solve_doppler_velocity_from_observations(
    observations: &[DopplerVelocityObservation],
    receiver_ecef: Ecef,
) -> VelocitySolution {
    if observations.len() < 4 {
        return VelocitySolution {
            used_prns: observations.iter().map(|obs| obs.prn).collect(),
            ..Default::default()
        };
    }

    let n = observations.len();
    let used_prns = observations.iter().map(|obs| obs.prn).collect::<Vec<_>>();
    let mut state = Vector4::<f64>::zeros();

    for _ in 0..4 {
        let mut residuals = DVector::<f64>::zeros(n);
        let mut h = DMatrix::<f64>::zeros(n, 4);
        for (row, obs) in observations.iter().enumerate() {
            let los = normalize(obs.line_of_sight);
            let observed_range_rate = -GPS_L1_WAVELENGTH_M * obs.doppler_hz;
            residuals[row] = observed_range_rate
                - predicted_range_rate(
                    obs,
                    receiver_ecef,
                    [state[0], state[1], state[2]],
                    state[3],
                );
            h[(row, 0)] = -los[0];
            h[(row, 1)] = -los[1];
            h[(row, 2)] = -los[2];
            h[(row, 3)] = 1.0;
        }

        let ht = h.transpose();
        let normal: Matrix4<f64> = (&ht * &h).fixed_resize::<4, 4>(0.0);
        let rhs: Vector4<f64> = (&ht * residuals).fixed_resize::<4, 1>(0.0);
        let Some(delta) = normal.try_inverse().map(|inverse| inverse * rhs) else {
            return VelocitySolution {
                used_prns,
                ..Default::default()
            };
        };
        state += delta;
        if delta.norm() < 1e-7 {
            break;
        }
    }

    let ecef_velocity_mps = [state[0], state[1], state[2]];
    let clock_drift_mps = state[3];

    let residuals_mps = observations
        .iter()
        .map(|obs| {
            let observed_range_rate = -GPS_L1_WAVELENGTH_M * obs.doppler_hz;
            let predicted =
                predicted_range_rate(obs, receiver_ecef, ecef_velocity_mps, clock_drift_mps);
            observed_range_rate - predicted
        })
        .collect();

    VelocitySolution {
        ecef_velocity_mps,
        enu_velocity_mps: ecef_velocity_to_enu(receiver_ecef, ecef_velocity_mps),
        clock_drift_mps,
        clock_drift_sps: clock_drift_mps / C_LIGHT,
        used_prns,
        residuals_mps,
        converged: true,
    }
}

pub fn doppler_velocity_observations(
    pseudoranges: &[Pseudorange],
    ephemerides: &EphemerisMap,
    receiver_ecef: Ecef,
) -> Vec<DopplerVelocityObservation> {
    pseudoranges
        .iter()
        .filter_map(|pr| {
            let eph = ephemerides.get(&pr.prn)?;
            let sat = satellite_position(eph, pr.transmit_time_sow);
            let sat_vel = satellite_velocity_finite_difference(eph, pr.transmit_time_sow);
            let line_of_sight = [
                sat.ecef[0] - receiver_ecef[0],
                sat.ecef[1] - receiver_ecef[1],
                sat.ecef[2] - receiver_ecef[2],
            ];
            Some(DopplerVelocityObservation {
                prn: pr.prn,
                line_of_sight: normalize(line_of_sight),
                satellite_velocity_mps: sat_vel,
                satellite_ecef: sat.ecef,
                satellite_clock_drift_sps: satellite_clock_drift(eph, pr.transmit_time_sow),
                doppler_hz: pr.doppler_hz,
                cn0_db_hz: pr.cn0_db_hz,
            })
        })
        .collect()
}

fn predicted_range_rate(
    observation: &DopplerVelocityObservation,
    receiver_ecef: Ecef,
    receiver_velocity_mps: Ecef,
    receiver_clock_drift_mps: f64,
) -> f64 {
    let los = normalize(observation.line_of_sight);
    let relative_velocity = [
        observation.satellite_velocity_mps[0] - receiver_velocity_mps[0],
        observation.satellite_velocity_mps[1] - receiver_velocity_mps[1],
        observation.satellite_velocity_mps[2] - receiver_velocity_mps[2],
    ];
    let earth_rotation = OMEGA_E / C_LIGHT
        * (observation.satellite_velocity_mps[1] * receiver_ecef[0]
            + observation.satellite_ecef[1] * receiver_velocity_mps[0]
            - observation.satellite_velocity_mps[0] * receiver_ecef[1]
            - observation.satellite_ecef[0] * receiver_velocity_mps[1]);
    dot(los, relative_velocity) + earth_rotation + receiver_clock_drift_mps
        - C_LIGHT * observation.satellite_clock_drift_sps
}

fn satellite_velocity_finite_difference(eph: &Ephemeris, transmit_time_sow: f64) -> Ecef {
    let dt = 0.5;
    let before = satellite_position(eph, transmit_time_sow - dt).ecef;
    let after = satellite_position(eph, transmit_time_sow + dt).ecef;
    [
        (after[0] - before[0]) / (2.0 * dt),
        (after[1] - before[1]) / (2.0 * dt),
        (after[2] - before[2]) / (2.0 * dt),
    ]
}

fn ecef_velocity_to_enu(receiver_ecef: Ecef, velocity_ecef: Ecef) -> Ecef {
    let lla = ecef_to_lla(receiver_ecef);
    let lat = lla[0];
    let lon = lla[1];
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let sin_lon = lon.sin();
    let cos_lon = lon.cos();
    [
        -sin_lon * velocity_ecef[0] + cos_lon * velocity_ecef[1],
        -sin_lat * cos_lon * velocity_ecef[0] - sin_lat * sin_lon * velocity_ecef[1]
            + cos_lat * velocity_ecef[2],
        cos_lat * cos_lon * velocity_ecef[0]
            + cos_lat * sin_lon * velocity_ecef[1]
            + sin_lat * velocity_ecef[2],
    ]
}

fn dot(a: Ecef, b: Ecef) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: Ecef) -> Ecef {
    let norm = dot(v, v).sqrt();
    if norm <= 0.0 {
        return [0.0; 3];
    }
    [v[0] / norm, v[1] / norm, v[2] / norm]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doppler_velocity_solver_recovers_synthetic_state() {
        let rx_velocity = [12.0, -4.0, 1.5];
        let clock_drift = 3.25;
        let observations = [
            make_obs(
                1,
                [1.0, 0.0, 0.0],
                [200.0, 10.0, -5.0],
                rx_velocity,
                clock_drift,
            ),
            make_obs(
                2,
                [0.0, 1.0, 0.0],
                [-50.0, 250.0, 8.0],
                rx_velocity,
                clock_drift,
            ),
            make_obs(
                3,
                [0.0, 0.0, 1.0],
                [12.0, -15.0, 300.0],
                rx_velocity,
                clock_drift,
            ),
            make_obs(
                4,
                normalize([1.0, 1.0, 1.0]),
                [140.0, 160.0, 180.0],
                rx_velocity,
                clock_drift,
            ),
            make_obs(
                5,
                normalize([-1.0, 2.0, 0.5]),
                [80.0, -60.0, 90.0],
                rx_velocity,
                clock_drift,
            ),
        ];

        let solution = solve_doppler_velocity_from_observations(&observations, [0.0; 3]);

        assert!(solution.converged);
        for (actual, expected) in solution.ecef_velocity_mps.iter().zip(rx_velocity.iter()) {
            assert!((actual - expected).abs() < 1e-9);
        }
        assert!((solution.clock_drift_mps - clock_drift).abs() < 1e-9);
        assert!(
            solution
                .residuals_mps
                .iter()
                .all(|residual| residual.abs() < 1e-9),
            "residuals: {:?}",
            solution.residuals_mps
        );
    }

    #[test]
    fn test_doppler_velocity_solver_requires_four_observations() {
        let observations = [
            make_obs(1, [1.0, 0.0, 0.0], [0.0; 3], [0.0; 3], 0.0),
            make_obs(2, [0.0, 1.0, 0.0], [0.0; 3], [0.0; 3], 0.0),
            make_obs(3, [0.0, 0.0, 1.0], [0.0; 3], [0.0; 3], 0.0),
        ];

        let solution = solve_doppler_velocity_from_observations(&observations, [0.0; 3]);

        assert!(!solution.converged);
        assert_eq!(solution.used_prns, vec![1, 2, 3]);
    }

    #[test]
    fn test_doppler_velocity_solver_uses_more_than_twelve_observations() {
        let rx_velocity = [12.0, -4.0, 1.5];
        let clock_drift = 3.25;
        let observations: Vec<DopplerVelocityObservation> = (1..=13)
            .map(|prn| {
                let phase = prn as f64 * 0.37;
                make_obs(
                    prn,
                    normalize([phase.cos(), phase.sin(), 0.2 + prn as f64 * 0.03]),
                    [
                        120.0 + prn as f64,
                        -80.0 + prn as f64 * 0.5,
                        40.0 - prn as f64 * 0.25,
                    ],
                    rx_velocity,
                    clock_drift,
                )
            })
            .collect();

        let solution = solve_doppler_velocity_from_observations(&observations, [0.0; 3]);

        assert!(solution.converged);
        assert_eq!(solution.used_prns.len(), 13);
        assert_eq!(solution.residuals_mps.len(), 13);
        assert_eq!(solution.used_prns.last(), Some(&13));
        for (actual, expected) in solution.ecef_velocity_mps.iter().zip(rx_velocity.iter()) {
            assert!((actual - expected).abs() < 1e-8);
        }
        assert!((solution.clock_drift_mps - clock_drift).abs() < 1e-8);
    }

    #[test]
    fn test_doppler_velocity_matches_rtklib_unweighted_lsq() {
        let receiver_velocity = [3.0, -2.0, 0.5];
        let clock_drift = -12.0;
        let mut observations = vec![
            make_obs(
                1,
                [1.0, 0.0, 0.0],
                [200.0, 10.0, -5.0],
                receiver_velocity,
                clock_drift,
            ),
            make_obs(
                2,
                [0.0, 1.0, 0.0],
                [-50.0, 250.0, 8.0],
                receiver_velocity,
                clock_drift,
            ),
            make_obs(
                3,
                [0.0, 0.0, 1.0],
                [20.0, -30.0, 180.0],
                receiver_velocity,
                clock_drift,
            ),
            make_obs(
                4,
                normalize([1.0, 1.0, 1.0]),
                [120.0, 90.0, 60.0],
                receiver_velocity,
                clock_drift,
            ),
            make_obs(
                5,
                normalize([-0.4, 0.8, 0.3]),
                [-80.0, 140.0, 30.0],
                receiver_velocity,
                clock_drift,
            ),
        ];
        observations[4].doppler_hz += 1.0;

        let reference = solve_doppler_velocity_from_observations(&observations, [0.0; 3]);
        observations[0].cn0_db_hz = 20.0;
        observations[1].cn0_db_hz = 60.0;
        observations[2].cn0_db_hz = 25.0;
        observations[3].cn0_db_hz = 55.0;
        observations[4].cn0_db_hz = 30.0;
        let varied_cn0 = solve_doppler_velocity_from_observations(&observations, [0.0; 3]);

        assert_eq!(reference.ecef_velocity_mps, varied_cn0.ecef_velocity_mps);
        assert_eq!(reference.clock_drift_mps, varied_cn0.clock_drift_mps);
    }

    fn make_obs(
        prn: u8,
        line_of_sight: Ecef,
        satellite_velocity_mps: Ecef,
        receiver_velocity_mps: Ecef,
        clock_drift_mps: f64,
    ) -> DopplerVelocityObservation {
        let range_rate = dot(line_of_sight, satellite_velocity_mps)
            - dot(line_of_sight, receiver_velocity_mps)
            + clock_drift_mps;
        DopplerVelocityObservation {
            prn,
            line_of_sight,
            satellite_velocity_mps,
            satellite_ecef: [0.0; 3],
            satellite_clock_drift_sps: 0.0,
            doppler_hz: -range_rate / GPS_L1_WAVELENGTH_M,
            cn0_db_hz: 45.0,
        }
    }
}
