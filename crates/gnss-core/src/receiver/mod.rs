//! Receiver orchestration primitives.
//!
//! These types keep dynamic acquisition, reacquisition, channel admission, and
//! acquisition buffering deterministic. They are intentionally independent from
//! GUI workers and async runtimes.

use crate::acquisition::AcquisitionResult;
use crate::tracking::TrackingOutput;
use num_complex::Complex;
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverChannelState {
    Absent,
    Acquiring,
    TrackingProbation,
    Healthy,
    Suspect,
    Lost,
    Reacquiring,
}

impl ReceiverChannelState {
    pub fn pvt_eligible(self) -> bool {
        matches!(self, Self::Healthy)
    }

    pub fn needs_reacquisition(self) -> bool {
        matches!(self, Self::Lost | Self::Reacquiring)
    }
}

#[derive(Debug, Clone)]
pub struct ChannelAdmissionConfig {
    pub probation_good_epochs: usize,
    pub suspect_bad_epochs: usize,
    pub lost_bad_epochs: usize,
    pub min_pll_lock_metric: f64,
    pub min_cn0_db_hz: f64,
    pub max_abs_dll_error_chips: f64,
    pub max_abs_pll_error_cycles: f64,
    pub max_doppler_jump_hz: f64,
    pub require_nav_time: bool,
    pub require_ephemeris: bool,
}

impl Default for ChannelAdmissionConfig {
    fn default() -> Self {
        Self {
            probation_good_epochs: 3,
            suspect_bad_epochs: 2,
            lost_bad_epochs: 5,
            min_pll_lock_metric: 0.65,
            min_cn0_db_hz: 25.0,
            max_abs_dll_error_chips: 0.25,
            max_abs_pll_error_cycles: 0.25,
            max_doppler_jump_hz: 500.0,
            require_nav_time: false,
            require_ephemeris: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChannelHealthObservation {
    pub pll_lock_metric: f64,
    pub cn0_db_hz: f64,
    pub dll_error_chips: f64,
    pub pll_error_cycles: f64,
    pub doppler_hz: f64,
    pub nav_time_available: bool,
    pub ephemeris_available: bool,
}

impl ChannelHealthObservation {
    pub fn from_tracking_output(
        output: &TrackingOutput,
        nav_time_available: bool,
        ephemeris_available: bool,
    ) -> Self {
        Self {
            pll_lock_metric: output.pll_lock_metric,
            cn0_db_hz: output.cn0_db_hz,
            dll_error_chips: output.dll_error,
            pll_error_cycles: 0.0,
            doppler_hz: output.carrier_doppler_hz,
            nav_time_available,
            ephemeris_available,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReceiverChannel {
    pub prn: u8,
    pub state: ReceiverChannelState,
    pub last_acquisition: Option<AcquisitionResult>,
    pub last_doppler_hz: Option<f64>,
    pub last_update_ms: Option<u64>,
    good_epochs: usize,
    bad_epochs: usize,
}

impl ReceiverChannel {
    fn new(prn: u8) -> Self {
        Self {
            prn,
            state: ReceiverChannelState::Absent,
            last_acquisition: None,
            last_doppler_hz: None,
            last_update_ms: None,
            good_epochs: 0,
            bad_epochs: 0,
        }
    }

    fn reset_counters(&mut self) {
        self.good_epochs = 0;
        self.bad_epochs = 0;
    }
}

#[derive(Debug, Clone)]
pub struct ChannelManager {
    channels: BTreeMap<u8, ReceiverChannel>,
    admission: ChannelAdmissionConfig,
}

impl ChannelManager {
    pub fn new<I>(prns: I, admission: ChannelAdmissionConfig) -> Self
    where
        I: IntoIterator<Item = u8>,
    {
        let channels = prns
            .into_iter()
            .map(|prn| (prn, ReceiverChannel::new(prn)))
            .collect();
        Self {
            channels,
            admission,
        }
    }

    pub fn gps_l1_ca(admission: ChannelAdmissionConfig) -> Self {
        Self::new(1..=32, admission)
    }

    pub fn channel(&self, prn: u8) -> Option<&ReceiverChannel> {
        self.channels.get(&prn)
    }

    pub fn state(&self, prn: u8) -> ReceiverChannelState {
        self.channels
            .get(&prn)
            .map(|channel| channel.state)
            .unwrap_or(ReceiverChannelState::Absent)
    }

    pub fn set_acquiring(&mut self, prn: u8, now_ms: u64) {
        let channel = self.ensure_channel(prn);
        channel.state = if channel.state.needs_reacquisition() {
            ReceiverChannelState::Reacquiring
        } else {
            ReceiverChannelState::Acquiring
        };
        channel.last_update_ms = Some(now_ms);
        channel.reset_counters();
    }

    pub fn accept_acquisition(&mut self, result: AcquisitionResult, now_ms: u64) {
        let channel = self.ensure_channel(result.prn);
        channel.state = ReceiverChannelState::TrackingProbation;
        channel.last_doppler_hz = Some(result.doppler_hz);
        channel.last_acquisition = Some(result);
        channel.last_update_ms = Some(now_ms);
        channel.reset_counters();
    }

    pub fn reject_acquisition(&mut self, prn: u8, now_ms: u64) {
        let channel = self.ensure_channel(prn);
        channel.state = if channel.state == ReceiverChannelState::Reacquiring {
            ReceiverChannelState::Lost
        } else {
            ReceiverChannelState::Absent
        };
        channel.last_update_ms = Some(now_ms);
        channel.reset_counters();
    }

    pub fn observe_tracking(
        &mut self,
        prn: u8,
        observation: ChannelHealthObservation,
        now_ms: u64,
    ) -> ReceiverChannelState {
        let admission = self.admission.clone();
        let channel = self.ensure_channel(prn);
        let healthy = channel_observation_is_healthy(channel, observation, &admission);
        channel.last_update_ms = Some(now_ms);
        channel.last_doppler_hz = Some(observation.doppler_hz);

        if healthy {
            channel.good_epochs += 1;
            channel.bad_epochs = 0;
            if (channel.state == ReceiverChannelState::TrackingProbation
                && channel.good_epochs >= admission.probation_good_epochs)
                || channel.state == ReceiverChannelState::Suspect
            {
                channel.state = ReceiverChannelState::Healthy;
            }
        } else {
            channel.bad_epochs += 1;
            channel.good_epochs = 0;
            if channel.bad_epochs >= admission.lost_bad_epochs {
                channel.state = ReceiverChannelState::Lost;
            } else if channel.state == ReceiverChannelState::Healthy
                && channel.bad_epochs >= admission.suspect_bad_epochs
            {
                channel.state = ReceiverChannelState::Suspect;
            }
        }

        channel.state
    }

    pub fn pvt_eligible_prns(&self) -> Vec<u8> {
        self.channels
            .iter()
            .filter_map(|(&prn, channel)| channel.state.pvt_eligible().then_some(prn))
            .collect()
    }

    pub fn reacquisition_prns(&self) -> Vec<u8> {
        self.channels
            .iter()
            .filter_map(|(&prn, channel)| {
                (channel.state == ReceiverChannelState::Lost).then_some(prn)
            })
            .collect()
    }

    pub fn absent_prns(&self) -> Vec<u8> {
        self.channels
            .iter()
            .filter_map(|(&prn, channel)| {
                matches!(channel.state, ReceiverChannelState::Absent).then_some(prn)
            })
            .collect()
    }

    fn ensure_channel(&mut self, prn: u8) -> &mut ReceiverChannel {
        self.channels
            .entry(prn)
            .or_insert_with(|| ReceiverChannel::new(prn))
    }
}

fn channel_observation_is_healthy(
    channel: &ReceiverChannel,
    observation: ChannelHealthObservation,
    admission: &ChannelAdmissionConfig,
) -> bool {
    if observation.pll_lock_metric < admission.min_pll_lock_metric
        || observation.cn0_db_hz < admission.min_cn0_db_hz
        || observation.dll_error_chips.abs() > admission.max_abs_dll_error_chips
        || observation.pll_error_cycles.abs() > admission.max_abs_pll_error_cycles
        || (admission.require_nav_time && !observation.nav_time_available)
        || (admission.require_ephemeris && !observation.ephemeris_available)
    {
        return false;
    }

    channel
        .last_doppler_hz
        .map(|last| (observation.doppler_hz - last).abs() <= admission.max_doppler_jump_hz)
        .unwrap_or(true)
}

#[derive(Debug, Clone)]
pub struct AcquisitionSchedulerConfig {
    pub cold_acq_interval_ms: u64,
    pub reacq_interval_ms: u64,
    pub max_acq_prns_per_scan: usize,
    pub acq_window_ms: usize,
    pub doppler_max_hz: f64,
    pub doppler_step_hz: f64,
    pub noncoherent_integrations: usize,
    pub fixed_prns: Option<Vec<u8>>,
}

impl Default for AcquisitionSchedulerConfig {
    fn default() -> Self {
        Self {
            cold_acq_interval_ms: 10_000,
            reacq_interval_ms: 1_000,
            max_acq_prns_per_scan: 4,
            acq_window_ms: 10,
            doppler_max_hz: 10_000.0,
            doppler_step_hz: 250.0,
            noncoherent_integrations: 10,
            fixed_prns: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquisitionScanKind {
    Cold,
    Reacquisition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionScanPlan {
    pub kind: AcquisitionScanKind,
    pub prns: Vec<u8>,
    pub window_ms: usize,
    pub noncoherent_integrations: usize,
}

#[derive(Debug, Clone)]
pub struct AcquisitionScheduler {
    config: AcquisitionSchedulerConfig,
    last_cold_scan_ms: Option<u64>,
    last_reacq_scan_ms: Option<u64>,
}

impl AcquisitionScheduler {
    pub fn new(config: AcquisitionSchedulerConfig) -> Self {
        Self {
            config,
            last_cold_scan_ms: None,
            last_reacq_scan_ms: None,
        }
    }

    pub fn config(&self) -> &AcquisitionSchedulerConfig {
        &self.config
    }

    pub fn plan_scan(
        &mut self,
        now_ms: u64,
        manager: &ChannelManager,
    ) -> Option<AcquisitionScanPlan> {
        if self.interval_due(
            self.last_reacq_scan_ms,
            now_ms,
            self.config.reacq_interval_ms,
        ) {
            let prns = limited(
                manager.reacquisition_prns(),
                self.config.max_acq_prns_per_scan,
            );
            if !prns.is_empty() {
                self.last_reacq_scan_ms = Some(now_ms);
                return Some(self.plan(AcquisitionScanKind::Reacquisition, prns));
            }
        }

        if self.interval_due(
            self.last_cold_scan_ms,
            now_ms,
            self.config.cold_acq_interval_ms,
        ) {
            let prns = limited(
                self.cold_candidates(manager),
                self.config.max_acq_prns_per_scan,
            );
            if !prns.is_empty() {
                self.last_cold_scan_ms = Some(now_ms);
                return Some(self.plan(AcquisitionScanKind::Cold, prns));
            }
        }

        None
    }

    fn cold_candidates(&self, manager: &ChannelManager) -> Vec<u8> {
        if let Some(fixed_prns) = &self.config.fixed_prns {
            fixed_prns
                .iter()
                .copied()
                .filter(|prn| manager.state(*prn) == ReceiverChannelState::Absent)
                .collect()
        } else {
            manager.absent_prns()
        }
    }

    fn plan(&self, kind: AcquisitionScanKind, prns: Vec<u8>) -> AcquisitionScanPlan {
        AcquisitionScanPlan {
            kind,
            prns,
            window_ms: self.config.acq_window_ms,
            noncoherent_integrations: self.config.noncoherent_integrations,
        }
    }

    fn interval_due(&self, last_ms: Option<u64>, now_ms: u64, interval_ms: u64) -> bool {
        last_ms
            .map(|last| now_ms.saturating_sub(last) >= interval_ms)
            .unwrap_or(true)
    }
}

fn limited(mut prns: Vec<u8>, max_len: usize) -> Vec<u8> {
    prns.sort_unstable();
    prns.truncate(max_len);
    prns
}

#[derive(Debug, Clone)]
pub struct AcquisitionRingBuffer {
    capacity_samples: usize,
    start_sample_index: u64,
    next_sample_index: u64,
    samples: VecDeque<Complex<f64>>,
}

impl AcquisitionRingBuffer {
    pub fn new(capacity_samples: usize, start_sample_index: u64) -> Self {
        Self {
            capacity_samples,
            start_sample_index,
            next_sample_index: start_sample_index,
            samples: VecDeque::with_capacity(capacity_samples),
        }
    }

    pub fn push_samples(&mut self, samples: &[Complex<f64>]) {
        for sample in samples {
            if self.samples.len() == self.capacity_samples {
                self.samples.pop_front();
                self.start_sample_index += 1;
            }
            self.samples.push_back(*sample);
            self.next_sample_index += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn start_sample_index(&self) -> u64 {
        self.start_sample_index
    }

    pub fn next_sample_index(&self) -> u64 {
        self.next_sample_index
    }

    pub fn latest_window(&self, count: usize) -> Option<(u64, Vec<Complex<f64>>)> {
        if count > self.samples.len() {
            return None;
        }
        let start_offset = self.samples.len() - count;
        let sample_index = self.start_sample_index + start_offset as u64;
        let window = self.samples.iter().skip(start_offset).copied().collect();
        Some((sample_index, window))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acq(prn: u8, doppler_hz: f64) -> AcquisitionResult {
        AcquisitionResult {
            prn,
            sample_index: 123,
            doppler_hz,
            code_phase_chips: 10.0,
            peak_to_mean: 20.0,
            detected: true,
        }
    }

    fn good_obs(doppler_hz: f64) -> ChannelHealthObservation {
        ChannelHealthObservation {
            pll_lock_metric: 0.9,
            cn0_db_hz: 42.0,
            dll_error_chips: 0.02,
            pll_error_cycles: 0.01,
            doppler_hz,
            nav_time_available: true,
            ephemeris_available: true,
        }
    }

    #[test]
    fn health_observation_maps_tracking_output_lock_fields() {
        let output = TrackingOutput {
            prompt: Complex::new(12.0, 1.0),
            early_mag: 11.0,
            late_mag: 10.0,
            code_phase_chips: 42.0,
            carrier_doppler_hz: 7_250.0,
            cn0_db_hz: 43.0,
            pll_lock_metric: 0.87,
            sample_counter: 4_000,
            tracking_sample_counter: 4_123,
            rem_code_phase_samples: 0.25,
            ms_within_bit: 3,
            dll_error: 0.03,
        };

        let observation = ChannelHealthObservation::from_tracking_output(&output, true, false);

        assert_eq!(observation.pll_lock_metric, 0.87);
        assert_eq!(observation.cn0_db_hz, 43.0);
        assert_eq!(observation.dll_error_chips, 0.03);
        assert_eq!(observation.doppler_hz, 7_250.0);
        assert!(observation.nav_time_available);
        assert!(!observation.ephemeris_available);
    }

    #[test]
    fn probation_promotes_to_pvt_eligible_healthy_channel() {
        let admission = ChannelAdmissionConfig {
            probation_good_epochs: 2,
            ..Default::default()
        };
        let mut manager = ChannelManager::new([11], admission);
        manager.accept_acquisition(acq(11, 5_500.0), 10);

        assert_eq!(manager.state(11), ReceiverChannelState::TrackingProbation);
        assert!(manager.pvt_eligible_prns().is_empty());

        manager.observe_tracking(11, good_obs(5_510.0), 20);
        assert_eq!(manager.state(11), ReceiverChannelState::TrackingProbation);

        manager.observe_tracking(11, good_obs(5_520.0), 30);
        assert_eq!(manager.state(11), ReceiverChannelState::Healthy);
        assert_eq!(manager.pvt_eligible_prns(), vec![11]);
    }

    #[test]
    fn repeated_bad_lock_moves_healthy_channel_to_lost() {
        let admission = ChannelAdmissionConfig {
            probation_good_epochs: 1,
            suspect_bad_epochs: 1,
            lost_bad_epochs: 3,
            ..Default::default()
        };
        let mut manager = ChannelManager::new([20], admission);
        manager.accept_acquisition(acq(20, 8_250.0), 0);
        manager.observe_tracking(20, good_obs(8_250.0), 1);
        assert_eq!(manager.state(20), ReceiverChannelState::Healthy);

        let mut bad = good_obs(8_250.0);
        bad.pll_lock_metric = 0.1;

        assert_eq!(
            manager.observe_tracking(20, bad, 2),
            ReceiverChannelState::Suspect
        );
        assert_eq!(
            manager.observe_tracking(20, bad, 3),
            ReceiverChannelState::Suspect
        );
        assert_eq!(
            manager.observe_tracking(20, bad, 4),
            ReceiverChannelState::Lost
        );
        assert_eq!(manager.reacquisition_prns(), vec![20]);
    }

    #[test]
    fn scheduler_prioritizes_reacquisition_before_cold_scan() {
        let admission = ChannelAdmissionConfig {
            probation_good_epochs: 1,
            lost_bad_epochs: 1,
            ..Default::default()
        };
        let mut manager = ChannelManager::new([1, 11, 17], admission);
        manager.accept_acquisition(acq(11, 5_500.0), 0);
        let mut bad = good_obs(5_500.0);
        bad.cn0_db_hz = 5.0;
        manager.observe_tracking(11, bad, 1);

        let mut scheduler = AcquisitionScheduler::new(AcquisitionSchedulerConfig {
            cold_acq_interval_ms: 1,
            reacq_interval_ms: 1,
            max_acq_prns_per_scan: 2,
            fixed_prns: Some(vec![1, 11, 17]),
            ..Default::default()
        });

        let plan = scheduler.plan_scan(1, &manager).expect("reacq plan");
        assert_eq!(plan.kind, AcquisitionScanKind::Reacquisition);
        assert_eq!(plan.prns, vec![11]);
        for prn in &plan.prns {
            manager.set_acquiring(*prn, 1);
        }

        let plan = scheduler.plan_scan(2, &manager).expect("cold plan");
        assert_eq!(plan.kind, AcquisitionScanKind::Cold);
        assert_eq!(plan.prns, vec![1, 17]);
    }

    #[test]
    fn ring_buffer_keeps_latest_samples_with_absolute_index() {
        let mut ring = AcquisitionRingBuffer::new(4, 100);
        ring.push_samples(&[
            Complex::new(1.0, 0.0),
            Complex::new(2.0, 0.0),
            Complex::new(3.0, 0.0),
        ]);
        assert_eq!(ring.latest_window(2).unwrap().0, 101);

        ring.push_samples(&[
            Complex::new(4.0, 0.0),
            Complex::new(5.0, 0.0),
            Complex::new(6.0, 0.0),
        ]);

        assert_eq!(ring.start_sample_index(), 102);
        assert_eq!(ring.next_sample_index(), 106);
        let (sample_index, window) = ring.latest_window(3).unwrap();
        assert_eq!(sample_index, 103);
        let re: Vec<f64> = window.iter().map(|sample| sample.re).collect();
        assert_eq!(re, vec![4.0, 5.0, 6.0]);
    }
}
