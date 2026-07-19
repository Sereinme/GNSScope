//! Lightweight continuous PVT Kalman filter.
//!
//! State is `[x, y, z, vx, vy, vz, clock_bias_m, clock_drift_mps]`.

use crate::pvt::coords::{ecef_to_lla, Ecef, Lla};
use crate::pvt::solver::PvtSolution;
use crate::pvt::velocity::VelocitySolution;
use nalgebra::{DMatrix, DVector};

const C_LIGHT: f64 = 299_792_458.0;

#[derive(Debug, Clone)]
pub struct KalmanConfig {
    pub initialization_epochs: usize,
    pub initial_position_sigma_m: f64,
    pub initial_velocity_sigma_mps: f64,
    pub initial_clock_bias_sigma_m: f64,
    pub initial_clock_drift_sigma_mps: f64,
    pub process_accel_sigma_mps2: f64,
    pub process_clock_drift_sigma_mps2: f64,
    pub position_measurement_sigma_m: f64,
    pub clock_bias_measurement_sigma_m: f64,
    pub velocity_measurement_sigma_mps: f64,
    pub clock_drift_measurement_sigma_mps: f64,
    pub adaptive_measurement_noise: bool,
    pub position_measurement_sigma_min_m: f64,
    pub position_measurement_sigma_max_m: f64,
    pub velocity_measurement_sigma_min_mps: f64,
    pub velocity_measurement_sigma_max_mps: f64,
    pub innovation_gate_sigma: f64,
    pub innovation_noise_inflation_max: f64,
}

impl Default for KalmanConfig {
    fn default() -> Self {
        Self {
            initialization_epochs: 1,
            initial_position_sigma_m: 100.0,
            initial_velocity_sigma_mps: 10.0,
            initial_clock_bias_sigma_m: 10_000.0,
            initial_clock_drift_sigma_mps: 1_000.0,
            process_accel_sigma_mps2: 2.0,
            process_clock_drift_sigma_mps2: 50.0,
            position_measurement_sigma_m: 25.0,
            clock_bias_measurement_sigma_m: 100.0,
            velocity_measurement_sigma_mps: 2.0,
            clock_drift_measurement_sigma_mps: 20.0,
            adaptive_measurement_noise: true,
            position_measurement_sigma_min_m: 3.0,
            position_measurement_sigma_max_m: 250.0,
            velocity_measurement_sigma_min_mps: 0.1,
            velocity_measurement_sigma_max_mps: 50.0,
            innovation_gate_sigma: 6.0,
            innovation_noise_inflation_max: 100.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KalmanMeasurementNoise {
    pub position_sigma_m: f64,
    pub position_covariance_ecef_m2: Option<[[f64; 3]; 3]>,
    pub clock_bias_sigma_m: f64,
    pub velocity_sigma_mps: f64,
    pub clock_drift_sigma_mps: f64,
}

#[derive(Debug, Clone)]
pub struct KalmanSolution {
    pub ecef: Ecef,
    pub lla: Lla,
    pub ecef_velocity_mps: Ecef,
    pub enu_velocity_mps: Ecef,
    pub clock_bias_s: f64,
    pub clock_drift_sps: f64,
    pub position_std_m: f64,
    pub velocity_std_mps: f64,
    pub position_measurement_sigma_m: f64,
    pub velocity_measurement_sigma_mps: f64,
    pub epoch_time_sow: f64,
    pub converged: bool,
}

impl Default for KalmanSolution {
    fn default() -> Self {
        Self {
            ecef: [0.0; 3],
            lla: [0.0; 3],
            ecef_velocity_mps: [0.0; 3],
            enu_velocity_mps: [0.0; 3],
            clock_bias_s: 0.0,
            clock_drift_sps: 0.0,
            position_std_m: 0.0,
            velocity_std_mps: 0.0,
            position_measurement_sigma_m: 0.0,
            velocity_measurement_sigma_mps: 0.0,
            epoch_time_sow: 0.0,
            converged: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PositionVelocityKalman {
    config: KalmanConfig,
    state: DVector<f64>,
    covariance: DMatrix<f64>,
    last_time_sow: Option<f64>,
    last_measurement_noise: KalmanMeasurementNoise,
    initialization_buffer: Vec<KalmanInitializationSample>,
    initialized: bool,
}

#[derive(Debug, Clone)]
struct KalmanInitializationSample {
    epoch_time_sow: f64,
    ecef: Ecef,
    clock_bias_m: f64,
    ecef_velocity_mps: Option<Ecef>,
    clock_drift_mps: Option<f64>,
    measurement_noise: KalmanMeasurementNoise,
}

impl PositionVelocityKalman {
    pub fn new(config: KalmanConfig) -> Self {
        let last_measurement_noise = KalmanMeasurementNoise {
            position_sigma_m: config.position_measurement_sigma_m,
            position_covariance_ecef_m2: None,
            clock_bias_sigma_m: config.clock_bias_measurement_sigma_m,
            velocity_sigma_mps: config.velocity_measurement_sigma_mps,
            clock_drift_sigma_mps: config.clock_drift_measurement_sigma_mps,
        };
        Self {
            config,
            state: DVector::zeros(8),
            covariance: DMatrix::identity(8, 8),
            last_time_sow: None,
            last_measurement_noise,
            initialization_buffer: Vec::new(),
            initialized: false,
        }
    }

    pub fn update(
        &mut self,
        epoch_time_sow: f64,
        position: &PvtSolution,
        velocity: Option<&VelocitySolution>,
    ) -> Option<KalmanSolution> {
        if !position.converged {
            return None;
        }
        let measurement_noise = self.measurement_noise(position, velocity);
        self.last_measurement_noise = measurement_noise;

        if !self.initialized {
            return self.initialize_or_buffer(
                epoch_time_sow,
                position,
                velocity,
                measurement_noise,
            );
        }

        let dt = self
            .last_time_sow
            .map(|last| (epoch_time_sow - last).clamp(0.0, 10.0))
            .unwrap_or(0.0);
        self.predict(dt);
        self.measurement_update(position, velocity, measurement_noise)?;
        self.last_time_sow = Some(epoch_time_sow);
        Some(self.solution(epoch_time_sow))
    }

    fn initialize_or_buffer(
        &mut self,
        epoch_time_sow: f64,
        position: &PvtSolution,
        velocity: Option<&VelocitySolution>,
        measurement_noise: KalmanMeasurementNoise,
    ) -> Option<KalmanSolution> {
        self.initialization_buffer.push(KalmanInitializationSample {
            epoch_time_sow,
            ecef: position.ecef,
            clock_bias_m: position.clock_bias_s * C_LIGHT,
            ecef_velocity_mps: velocity
                .filter(|solution| solution.converged)
                .map(|solution| solution.ecef_velocity_mps),
            clock_drift_mps: velocity
                .filter(|solution| solution.converged)
                .map(|solution| solution.clock_drift_mps),
            measurement_noise,
        });

        let required_epochs = self.config.initialization_epochs.max(1);
        if self.initialization_buffer.len() < required_epochs {
            return None;
        }

        let sample = averaged_initialization_sample(&self.initialization_buffer)?;
        self.initialize_from_sample(&sample);
        self.initialization_buffer.clear();
        Some(self.solution(sample.epoch_time_sow))
    }

    fn initialize_from_sample(&mut self, sample: &KalmanInitializationSample) {
        self.state.fill(0.0);
        self.state[0] = sample.ecef[0];
        self.state[1] = sample.ecef[1];
        self.state[2] = sample.ecef[2];
        if let Some(velocity) = sample.ecef_velocity_mps {
            self.state[3] = velocity[0];
            self.state[4] = velocity[1];
            self.state[5] = velocity[2];
        }
        self.state[6] = sample.clock_bias_m;
        if let Some(clock_drift_mps) = sample.clock_drift_mps {
            self.state[7] = clock_drift_mps;
        }

        self.covariance = DMatrix::zeros(8, 8);
        for i in 0..3 {
            self.covariance[(i, i)] = self.config.initial_position_sigma_m.powi(2);
            self.covariance[(i + 3, i + 3)] = self.config.initial_velocity_sigma_mps.powi(2);
        }
        self.covariance[(6, 6)] = self.config.initial_clock_bias_sigma_m.powi(2);
        self.covariance[(7, 7)] = self.config.initial_clock_drift_sigma_mps.powi(2);
        self.last_time_sow = Some(sample.epoch_time_sow);
        self.last_measurement_noise = sample.measurement_noise;
        self.initialized = true;
    }

    fn predict(&mut self, dt: f64) {
        let mut f = DMatrix::identity(8, 8);
        for i in 0..3 {
            f[(i, i + 3)] = dt;
        }
        f[(6, 7)] = dt;

        let mut q = DMatrix::zeros(8, 8);
        let accel_var = self.config.process_accel_sigma_mps2.powi(2);
        let clock_var = self.config.process_clock_drift_sigma_mps2.powi(2);
        let dt2 = dt * dt;
        let dt3 = dt2 * dt;
        let dt4 = dt2 * dt2;
        for i in 0..3 {
            q[(i, i)] = 0.25 * dt4 * accel_var;
            q[(i, i + 3)] = 0.5 * dt3 * accel_var;
            q[(i + 3, i)] = 0.5 * dt3 * accel_var;
            q[(i + 3, i + 3)] = dt2 * accel_var;
        }
        q[(6, 6)] = 0.25 * dt4 * clock_var;
        q[(6, 7)] = 0.5 * dt3 * clock_var;
        q[(7, 6)] = 0.5 * dt3 * clock_var;
        q[(7, 7)] = dt2 * clock_var;

        self.state = &f * &self.state;
        self.covariance = &f * &self.covariance * f.transpose() + q;
    }

    fn measurement_update(
        &mut self,
        position: &PvtSolution,
        velocity: Option<&VelocitySolution>,
        measurement_noise: KalmanMeasurementNoise,
    ) -> Option<()> {
        let has_velocity = velocity.is_some_and(|solution| solution.converged);
        let rows = if has_velocity { 8 } else { 4 };
        let mut z = DVector::zeros(rows);
        let mut h = DMatrix::zeros(rows, 8);
        let mut r = DMatrix::zeros(rows, rows);

        for i in 0..3 {
            z[i] = position.ecef[i];
            h[(i, i)] = 1.0;
            r[(i, i)] = measurement_noise.position_sigma_m.powi(2);
        }
        if let Some(position_covariance_ecef_m2) = measurement_noise.position_covariance_ecef_m2 {
            for row in 0..3 {
                for col in 0..3 {
                    r[(row, col)] = position_covariance_ecef_m2[row][col];
                }
            }
        }
        z[3] = position.clock_bias_s * C_LIGHT;
        h[(3, 6)] = 1.0;
        r[(3, 3)] = measurement_noise.clock_bias_sigma_m.powi(2);

        if let Some(velocity) = velocity.filter(|solution| solution.converged) {
            for i in 0..3 {
                z[i + 4] = velocity.ecef_velocity_mps[i];
                h[(i + 4, i + 3)] = 1.0;
                r[(i + 4, i + 4)] = measurement_noise.velocity_sigma_mps.powi(2);
            }
            z[7] = velocity.clock_drift_mps;
            h[(7, 7)] = 1.0;
            r[(7, 7)] = measurement_noise.clock_drift_sigma_mps.powi(2);
        }

        let innovation = z - &h * &self.state;
        self.inflate_measurement_noise_for_innovation(&innovation, &mut r, has_velocity);
        let s = &h * &self.covariance * h.transpose() + &r;
        let s_inv = s.try_inverse()?;
        let k = &self.covariance * h.transpose() * s_inv;
        self.state = &self.state + &k * innovation;
        let identity = DMatrix::identity(8, 8);
        let i_minus_kh = &identity - &k * &h;
        self.covariance =
            &i_minus_kh * &self.covariance * i_minus_kh.transpose() + &k * r * k.transpose();
        self.symmetrize_covariance();
        Some(())
    }

    fn inflate_measurement_noise_for_innovation(
        &self,
        innovation: &DVector<f64>,
        r: &mut DMatrix<f64>,
        has_velocity: bool,
    ) {
        self.inflate_block_for_innovation(innovation, r, 0, 3);
        if has_velocity {
            self.inflate_block_for_innovation(innovation, r, 4, 3);
        }
    }

    fn inflate_block_for_innovation(
        &self,
        innovation: &DVector<f64>,
        r: &mut DMatrix<f64>,
        start: usize,
        len: usize,
    ) {
        let gate = self.config.innovation_gate_sigma;
        let max_inflation = self.config.innovation_noise_inflation_max;
        if !gate.is_finite() || gate <= 0.0 || !max_inflation.is_finite() || max_inflation <= 1.0 {
            return;
        }

        let normalized = (start..start + len)
            .map(|row| {
                let variance = r[(row, row)].max(1e-12);
                innovation[row] * innovation[row] / variance
            })
            .sum::<f64>()
            .sqrt();
        if normalized <= gate {
            return;
        }

        let inflation = (normalized / gate).powi(2).clamp(1.0, max_inflation);
        for row in start..start + len {
            r[(row, row)] *= inflation;
        }
    }

    fn symmetrize_covariance(&mut self) {
        for row in 0..self.covariance.nrows() {
            for col in row + 1..self.covariance.ncols() {
                let value = 0.5 * (self.covariance[(row, col)] + self.covariance[(col, row)]);
                self.covariance[(row, col)] = value;
                self.covariance[(col, row)] = value;
            }
        }
    }

    fn measurement_noise(
        &self,
        position: &PvtSolution,
        velocity: Option<&VelocitySolution>,
    ) -> KalmanMeasurementNoise {
        if !self.config.adaptive_measurement_noise {
            return KalmanMeasurementNoise {
                position_sigma_m: self.config.position_measurement_sigma_m,
                position_covariance_ecef_m2: None,
                clock_bias_sigma_m: self.config.clock_bias_measurement_sigma_m,
                velocity_sigma_mps: self.config.velocity_measurement_sigma_mps,
                clock_drift_sigma_mps: self.config.clock_drift_measurement_sigma_mps,
            };
        }

        let residual_rms_m = rms(&position.residuals_m).unwrap_or(0.0);
        let mean_pr_sigma_m = mean(
            position
                .satellite_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.pseudorange_sigma_m),
        )
        .unwrap_or(self.config.position_measurement_sigma_m);
        let dof_scale = if position.n_sats > 4 {
            (position.n_sats as f64 / (position.n_sats - 4) as f64).sqrt()
        } else {
            4.0
        };
        let coarse_position_sigma_m = mean_pr_sigma_m * dof_scale;
        let wls_position_sigma_m =
            position_measurement_sigma_from_wls(position).unwrap_or(coarse_position_sigma_m);
        let epoch_quality_scale = finite_positive(position.epoch_quality_scale)
            .unwrap_or(1.0)
            .max(1.0);
        let position_sigma_m = ((wls_position_sigma_m + residual_rms_m) * epoch_quality_scale)
            .clamp(
                self.config.position_measurement_sigma_min_m,
                self.config.position_measurement_sigma_max_m,
            );
        let position_covariance_ecef_m2 =
            normalized_position_covariance_ecef(position, position_sigma_m);
        let clock_bias_sigma_m = finite_positive(position.clock_bias_sigma_m)
            .unwrap_or(self.config.clock_bias_measurement_sigma_m)
            .max(position_sigma_m);

        let velocity_sigma_mps = velocity
            .filter(|solution| solution.converged)
            .and_then(|solution| rms(&solution.residuals_mps))
            .map(|sigma| {
                sigma.clamp(
                    self.config.velocity_measurement_sigma_min_mps,
                    self.config.velocity_measurement_sigma_max_mps,
                )
            })
            .unwrap_or(self.config.velocity_measurement_sigma_mps);

        KalmanMeasurementNoise {
            position_sigma_m,
            position_covariance_ecef_m2,
            clock_bias_sigma_m,
            velocity_sigma_mps,
            clock_drift_sigma_mps: self
                .config
                .clock_drift_measurement_sigma_mps
                .max(velocity_sigma_mps),
        }
    }

    fn solution(&self, epoch_time_sow: f64) -> KalmanSolution {
        let ecef = [self.state[0], self.state[1], self.state[2]];
        let ecef_velocity_mps = [self.state[3], self.state[4], self.state[5]];
        KalmanSolution {
            ecef,
            lla: ecef_to_lla(ecef),
            ecef_velocity_mps,
            enu_velocity_mps: ecef_velocity_to_enu(ecef, ecef_velocity_mps),
            clock_bias_s: self.state[6] / C_LIGHT,
            clock_drift_sps: self.state[7] / C_LIGHT,
            position_std_m: diagonal_rms(&self.covariance, 0),
            velocity_std_mps: diagonal_rms(&self.covariance, 3),
            position_measurement_sigma_m: self.last_measurement_noise.position_sigma_m,
            velocity_measurement_sigma_mps: self.last_measurement_noise.velocity_sigma_mps,
            epoch_time_sow,
            converged: true,
        }
    }
}

fn diagonal_rms(covariance: &DMatrix<f64>, offset: usize) -> f64 {
    ((0..3)
        .map(|i| covariance[(offset + i, offset + i)].max(0.0))
        .sum::<f64>()
        / 3.0)
        .sqrt()
}

fn rms(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some((values.iter().map(|value| value * value).sum::<f64>() / values.len() as f64).sqrt())
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;
    for value in values {
        sum += value;
        count += 1;
    }
    (count > 0).then_some(sum / count as f64)
}

fn finite_positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn position_measurement_sigma_from_wls(position: &PvtSolution) -> Option<f64> {
    [
        finite_positive(position.position_sigma_m),
        finite_positive(position.horizontal_sigma_m),
        finite_positive(position.vertical_sigma_m),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::max)
}

fn normalized_position_covariance_ecef(
    position: &PvtSolution,
    position_sigma_m: f64,
) -> Option<[[f64; 3]; 3]> {
    let covariance = position.position_covariance_ecef_m2;
    let trace = covariance[0][0] + covariance[1][1] + covariance[2][2];
    if !position_sigma_m.is_finite()
        || position_sigma_m <= 0.0
        || !trace.is_finite()
        || trace <= 0.0
    {
        return None;
    }
    if covariance.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }

    let scale = 3.0 * position_sigma_m.powi(2) / trace;
    let mut normalized = [[0.0; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            normalized[row][col] = 0.5 * (covariance[row][col] + covariance[col][row]) * scale;
        }
    }
    for (idx, row) in normalized.iter_mut().enumerate() {
        if row[idx] <= 0.0 || !row[idx].is_finite() {
            return None;
        }
        row[idx] = row[idx].max(1e-6);
    }
    Some(normalized)
}

fn averaged_initialization_sample(
    samples: &[KalmanInitializationSample],
) -> Option<KalmanInitializationSample> {
    if samples.is_empty() {
        return None;
    }

    let mut ecef = [0.0; 3];
    let mut velocity_sum = [0.0; 3];
    let mut velocity_count = 0usize;
    let mut clock_bias_m = 0.0;
    let mut clock_drift_mps = 0.0;
    let mut clock_drift_count = 0usize;
    let mut noise = KalmanMeasurementNoise {
        position_sigma_m: 0.0,
        position_covariance_ecef_m2: None,
        clock_bias_sigma_m: 0.0,
        velocity_sigma_mps: 0.0,
        clock_drift_sigma_mps: 0.0,
    };

    for sample in samples {
        for (sum, value) in ecef.iter_mut().zip(sample.ecef) {
            *sum += value;
        }
        if let Some(velocity) = sample.ecef_velocity_mps {
            for (sum, value) in velocity_sum.iter_mut().zip(velocity) {
                *sum += value;
            }
            velocity_count += 1;
        }
        clock_bias_m += sample.clock_bias_m;
        if let Some(drift) = sample.clock_drift_mps {
            clock_drift_mps += drift;
            clock_drift_count += 1;
        }
        noise.position_sigma_m += sample.measurement_noise.position_sigma_m;
        noise.clock_bias_sigma_m += sample.measurement_noise.clock_bias_sigma_m;
        noise.velocity_sigma_mps += sample.measurement_noise.velocity_sigma_mps;
        noise.clock_drift_sigma_mps += sample.measurement_noise.clock_drift_sigma_mps;
    }

    let n = samples.len() as f64;
    for value in &mut ecef {
        *value /= n;
    }
    let ecef_velocity_mps = (velocity_count > 0).then(|| {
        let n = velocity_count as f64;
        [
            velocity_sum[0] / n,
            velocity_sum[1] / n,
            velocity_sum[2] / n,
        ]
    });
    let clock_drift_mps =
        (clock_drift_count > 0).then_some(clock_drift_mps / clock_drift_count as f64);
    noise.position_sigma_m /= n;
    noise.clock_bias_sigma_m /= n;
    noise.velocity_sigma_mps /= n;
    noise.clock_drift_sigma_mps /= n;

    Some(KalmanInitializationSample {
        epoch_time_sow: samples.last()?.epoch_time_sow,
        ecef,
        clock_bias_m: clock_bias_m / n,
        ecef_velocity_mps,
        clock_drift_mps,
        measurement_noise: noise,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kalman_tracks_constant_velocity_position() {
        let mut filter = PositionVelocityKalman::new(KalmanConfig::default());
        let first = filter
            .update(
                100.0,
                &position_solution([1_000.0, 2_000.0, 3_000.0], 0.0),
                Some(&velocity_solution([5.0, -2.0, 1.0], 0.0)),
            )
            .unwrap();
        assert!(first.converged);

        let second = filter
            .update(
                101.0,
                &position_solution([1_005.5, 1_998.2, 3_001.2], 0.0),
                Some(&velocity_solution([5.0, -2.0, 1.0], 0.0)),
            )
            .unwrap();

        assert!(second.ecef[0] > first.ecef[0]);
        assert!((second.ecef_velocity_mps[0] - 5.0).abs() < 0.5);
        assert!(second.position_std_m < first.position_std_m);
    }

    #[test]
    fn test_kalman_skips_unconverged_position() {
        let mut filter = PositionVelocityKalman::new(KalmanConfig::default());
        let solution = PvtSolution {
            converged: false,
            ..Default::default()
        };
        assert!(filter.update(100.0, &solution, None).is_none());
    }

    #[test]
    fn test_kalman_warms_up_before_initializing() {
        let mut filter = PositionVelocityKalman::new(KalmanConfig {
            initialization_epochs: 2,
            ..Default::default()
        });

        let first = filter.update(
            100.0,
            &position_solution([1_000.0, 2_000.0, 3_000.0], 1e-6),
            Some(&velocity_solution([2.0, 4.0, 6.0], 10.0)),
        );
        assert!(first.is_none());

        let second = filter
            .update(
                101.0,
                &position_solution([1_010.0, 2_020.0, 3_030.0], 3e-6),
                Some(&velocity_solution([4.0, 6.0, 8.0], 30.0)),
            )
            .unwrap();

        assert!(second.converged);
        assert_eq!(second.epoch_time_sow, 101.0);
        assert!((second.ecef[0] - 1_005.0).abs() < 1e-9);
        assert!((second.ecef[1] - 2_010.0).abs() < 1e-9);
        assert!((second.ecef[2] - 3_015.0).abs() < 1e-9);
        assert!((second.ecef_velocity_mps[0] - 3.0).abs() < 1e-9);
        assert!((second.ecef_velocity_mps[1] - 5.0).abs() < 1e-9);
        assert!((second.ecef_velocity_mps[2] - 7.0).abs() < 1e-9);
        assert!((second.clock_bias_s - 2e-6).abs() < 1e-15);
        assert!((second.clock_drift_sps - 20.0 / C_LIGHT).abs() < 1e-15);
    }

    #[test]
    fn test_kalman_adaptive_measurement_noise_tracks_residual_quality() {
        let mut filter = PositionVelocityKalman::new(KalmanConfig {
            position_measurement_sigma_min_m: 1.0,
            velocity_measurement_sigma_min_mps: 0.05,
            ..Default::default()
        });
        let position = position_solution_with_quality(
            [1_000.0, 2_000.0, 3_000.0],
            vec![10.0, -20.0, 30.0, -40.0, 50.0],
            vec![5.0, 6.0, 7.0, 8.0, 9.0],
        );
        let velocity =
            velocity_solution_with_residuals([1.0, 0.0, 0.0], 0.0, vec![0.2, -0.4, 0.6, -0.8, 1.0]);

        let solution = filter.update(100.0, &position, Some(&velocity)).unwrap();

        assert!(solution.position_measurement_sigma_m > 15.0);
        assert!(solution.velocity_measurement_sigma_mps > 0.5);
    }

    #[test]
    fn test_kalman_adaptive_measurement_noise_prefers_conservative_wls_sigma() {
        let mut filter = PositionVelocityKalman::new(KalmanConfig {
            position_measurement_sigma_min_m: 1.0,
            ..Default::default()
        });
        let mut position =
            position_solution_with_quality([1_000.0, 2_000.0, 3_000.0], vec![0.0; 5], vec![2.0; 5]);
        position.position_sigma_m = 40.0;
        position.horizontal_sigma_m = 30.0;
        position.vertical_sigma_m = 70.0;
        position.clock_bias_sigma_m = 60.0;

        let solution = filter.update(100.0, &position, None).unwrap();

        assert!((solution.position_measurement_sigma_m - 70.0).abs() < 1e-12);
    }

    #[test]
    fn test_kalman_adaptive_measurement_noise_applies_epoch_quality_scale() {
        let mut filter = PositionVelocityKalman::new(KalmanConfig {
            position_measurement_sigma_min_m: 1.0,
            ..Default::default()
        });
        let mut position =
            position_solution_with_quality([1_000.0, 2_000.0, 3_000.0], vec![0.0; 5], vec![2.0; 5]);
        position.position_sigma_m = 20.0;
        position.horizontal_sigma_m = 15.0;
        position.vertical_sigma_m = 10.0;
        position.epoch_quality_scale = 2.5;

        let solution = filter.update(100.0, &position, None).unwrap();

        assert!((solution.position_measurement_sigma_m - 50.0).abs() < 1e-12);
    }

    #[test]
    fn test_normalized_position_covariance_preserves_shape_at_scalar_noise_level() {
        let mut position =
            position_solution_with_quality([1_000.0, 2_000.0, 3_000.0], vec![0.0; 5], vec![2.0; 5]);
        position.position_covariance_ecef_m2 = [[4.0, 1.0, 0.5], [1.0, 9.0, 1.5], [0.5, 1.5, 16.0]];

        let covariance = normalized_position_covariance_ecef(&position, 10.0).unwrap();
        let trace = covariance[0][0] + covariance[1][1] + covariance[2][2];

        assert!((trace - 300.0).abs() < 1e-9);
        assert_eq!(covariance[0][1], covariance[1][0]);
        assert_eq!(covariance[0][2], covariance[2][0]);
        assert_eq!(covariance[1][2], covariance[2][1]);
        assert!(covariance[2][2] > covariance[1][1]);
        assert!(covariance[1][1] > covariance[0][0]);
    }

    #[test]
    fn test_kalman_inflates_outlier_position_innovation() {
        let config = KalmanConfig {
            adaptive_measurement_noise: false,
            initial_position_sigma_m: 100.0,
            process_accel_sigma_mps2: 0.1,
            position_measurement_sigma_m: 1.0,
            clock_bias_measurement_sigma_m: 10.0,
            innovation_gate_sigma: 3.0,
            innovation_noise_inflation_max: 10_000.0,
            ..Default::default()
        };
        let mut gated = PositionVelocityKalman::new(config.clone());
        let mut ungated = PositionVelocityKalman::new(KalmanConfig {
            innovation_gate_sigma: f64::INFINITY,
            ..config
        });

        let initial = position_solution([0.0, 0.0, 0.0], 0.0);
        gated.update(100.0, &initial, None).unwrap();
        ungated.update(100.0, &initial, None).unwrap();

        let outlier = position_solution([1_000.0, 0.0, 0.0], 0.0);
        let gated_solution = gated.update(101.0, &outlier, None).unwrap();
        let ungated_solution = ungated.update(101.0, &outlier, None).unwrap();

        assert!(
            gated_solution.ecef[0] < ungated_solution.ecef[0] * 0.75,
            "gated filter should be less affected by the outlier: gated={} ungated={}",
            gated_solution.ecef[0],
            ungated_solution.ecef[0]
        );
        assert_covariance_is_symmetric(&gated);
    }

    fn position_solution(ecef: Ecef, clock_bias_s: f64) -> PvtSolution {
        PvtSolution {
            ecef,
            clock_bias_s,
            converged: true,
            ..Default::default()
        }
    }

    fn position_solution_with_quality(
        ecef: Ecef,
        residuals_m: Vec<f64>,
        sigmas_m: Vec<f64>,
    ) -> PvtSolution {
        let n_sats = residuals_m.len();
        PvtSolution {
            ecef,
            n_sats,
            residuals_m,
            satellite_diagnostics: sigmas_m
                .into_iter()
                .map(
                    |pseudorange_sigma_m| crate::pvt::solver::PvtSatelliteDiagnostic {
                        prn: 1,
                        residual_m: 0.0,
                        standardized_residual: 0.0,
                        azimuth_rad: 0.0,
                        elevation_rad: 0.0,
                        pseudorange_sigma_m,
                        cn0_sigma_m: pseudorange_sigma_m,
                        elevation_sigma_m: 0.0,
                        iono_sigma_m: 0.0,
                        tracking_jitter_sigma_m: 0.0,
                        weight: 1.0,
                        iono_delay_m: 0.0,
                        tropo_delay_m: 0.0,
                    },
                )
                .collect(),
            converged: true,
            ..Default::default()
        }
    }

    fn velocity_solution(ecef_velocity_mps: Ecef, clock_drift_mps: f64) -> VelocitySolution {
        VelocitySolution {
            ecef_velocity_mps,
            clock_drift_mps,
            converged: true,
            ..Default::default()
        }
    }

    fn velocity_solution_with_residuals(
        ecef_velocity_mps: Ecef,
        clock_drift_mps: f64,
        residuals_mps: Vec<f64>,
    ) -> VelocitySolution {
        VelocitySolution {
            ecef_velocity_mps,
            clock_drift_mps,
            residuals_mps,
            converged: true,
            ..Default::default()
        }
    }

    fn assert_covariance_is_symmetric(filter: &PositionVelocityKalman) {
        for row in 0..filter.covariance.nrows() {
            for col in 0..filter.covariance.ncols() {
                assert!(
                    (filter.covariance[(row, col)] - filter.covariance[(col, row)]).abs() < 1e-9,
                    "covariance is not symmetric at ({row}, {col})"
                );
            }
        }
    }
}
