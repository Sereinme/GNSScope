//! Receiver orchestration primitives.
//!
//! These types keep dynamic acquisition, reacquisition, channel admission, and
//! acquisition buffering deterministic. They are intentionally independent from
//! GUI workers and async runtimes.

use crate::acquisition::{AcquisitionResult, PcpsAcquisition, PcpsConfig};
use crate::io::{IqFormat, IqReader};
use crate::nav::RinexNavConfig;
use crate::pvt::{ContinuousPvtConfig, PvtEpochOutput};
use crate::tracking::{DllPllTracking, TrackingConfig, TrackingOutput};
use num_complex::Complex;
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::PathBuf;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiverChannelSnapshot {
    pub prn: u8,
    pub state: ReceiverChannelState,
    pub last_doppler_hz: Option<f64>,
    pub last_update_ms: Option<u64>,
    pub pvt_eligible: bool,
    pub tracking_active: bool,
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

    pub fn snapshots(&self, active_tracking_prns: &[u8]) -> Vec<ReceiverChannelSnapshot> {
        self.channels
            .values()
            .map(|channel| ReceiverChannelSnapshot {
                prn: channel.prn,
                state: channel.state,
                last_doppler_hz: channel.last_doppler_hz,
                last_update_ms: channel.last_update_ms,
                pvt_eligible: channel.state.pvt_eligible(),
                tracking_active: active_tracking_prns.binary_search(&channel.prn).is_ok(),
            })
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
    pub threshold: f64,
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
            threshold: 3.0,
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
pub struct StreamingPvtConfig {
    pub iq_path: Option<PathBuf>,
    pub iq_format: IqFormat,
    pub sample_rate_hz: f64,
    pub intermediate_frequency_hz: f64,
    pub chunk_ms: usize,
    pub pvt_output_rate_ms: u64,
    pub fixed_prns: Option<Vec<u8>>,
    pub rinex_nav: RinexNavConfig,
    pub acquisition: AcquisitionSchedulerConfig,
    pub channel_admission: ChannelAdmissionConfig,
    pub tracking: TrackingConfig,
    pub continuous_pvt: ContinuousPvtConfig,
    pub acquisition_ring_ms: usize,
}

impl Default for StreamingPvtConfig {
    fn default() -> Self {
        let acquisition = AcquisitionSchedulerConfig::default();
        Self {
            iq_path: None,
            iq_format: IqFormat::Int16,
            sample_rate_hz: 4_000_000.0,
            intermediate_frequency_hz: 0.0,
            chunk_ms: 1,
            pvt_output_rate_ms: 1_000,
            fixed_prns: None,
            rinex_nav: RinexNavConfig::default(),
            acquisition_ring_ms: acquisition.acq_window_ms.max(20),
            acquisition,
            channel_admission: ChannelAdmissionConfig::default(),
            tracking: TrackingConfig::default(),
            continuous_pvt: ContinuousPvtConfig::default(),
        }
    }
}

impl StreamingPvtConfig {
    pub fn cttc_fixed_prns(prns: Vec<u8>) -> Self {
        let acquisition = AcquisitionSchedulerConfig {
            fixed_prns: Some(prns.clone()),
            ..Default::default()
        };
        Self {
            fixed_prns: Some(prns),
            acquisition,
            ..Default::default()
        }
    }

    pub fn samples_per_ms(&self) -> usize {
        (self.sample_rate_hz / 1000.0).round() as usize
    }

    pub fn chunk_samples(&self) -> usize {
        self.chunk_ms.max(1) * self.samples_per_ms()
    }

    pub fn acquisition_ring_samples(&self) -> usize {
        self.acquisition_ring_ms.max(self.acquisition.acq_window_ms) * self.samples_per_ms()
    }

    pub fn acquisition_window_samples(&self) -> usize {
        self.acquisition.acq_window_ms * self.samples_per_ms()
    }

    pub fn with_output_rate_ms(mut self, output_rate_ms: u64) -> Self {
        self.pvt_output_rate_ms = output_rate_ms.max(1);
        self
    }

    pub fn with_chunk_ms(mut self, chunk_ms: usize) -> Self {
        self.chunk_ms = chunk_ms.max(1);
        self
    }
}

#[derive(Debug, Clone)]
pub struct StreamingPvtStateSnapshot {
    pub sample_index: u64,
    pub next_pvt_output_ms: u64,
    pub pvt_output_rate_ms: u64,
    pub channels: Vec<ReceiverChannelSnapshot>,
}

#[derive(Debug, Clone)]
pub struct StreamingAcquisitionScan {
    pub plan: AcquisitionScanPlan,
    pub sample_index: u64,
    pub sample_count: usize,
}

#[derive(Debug, Clone)]
pub struct StreamingAcquisitionScanResult {
    pub scan: StreamingAcquisitionScan,
    pub results: Vec<AcquisitionResult>,
}

#[derive(Debug, Clone)]
pub struct StreamingTrackingUpdate {
    pub prn: u8,
    pub now_ms: u64,
    pub output: TrackingOutput,
    pub state: ReceiverChannelState,
}

#[derive(Debug, Clone)]
pub enum StreamingPvtEvent {
    AcquisitionScanPlanned(StreamingAcquisitionScan),
    AcquisitionScanCompleted(StreamingAcquisitionScanResult),
    TrackingUpdated(StreamingTrackingUpdate),
    PvtOutput(Box<PvtEpochOutput>),
}

#[derive(Debug)]
pub enum StreamingPvtError {
    MissingIqPath,
    Io(io::Error),
}

impl From<io::Error> for StreamingPvtError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct StreamingPvtReadStep {
    pub start_sample_index: u64,
    pub samples_read: usize,
    pub now_ms: u64,
    pub events: Vec<StreamingPvtEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct StreamingPvtRunSummary {
    pub samples_read: usize,
    pub steps: usize,
    pub events_emitted: usize,
}

impl StreamingPvtRunSummary {
    pub fn ms_read(&self, sample_rate_hz: f64) -> f64 {
        self.samples_read as f64 * 1000.0 / sample_rate_hz
    }
}

pub struct StreamingPvtRunner {
    config: StreamingPvtConfig,
    channels: ChannelManager,
    acquisition_scheduler: AcquisitionScheduler,
    acquisition_ring: AcquisitionRingBuffer,
    active_tracking: BTreeMap<u8, DllPllTracking>,
    next_pvt_output_ms: u64,
}

impl StreamingPvtRunner {
    pub fn new(config: StreamingPvtConfig) -> Self {
        let prns = config
            .fixed_prns
            .clone()
            .unwrap_or_else(|| (1..=32).collect());
        let channels = ChannelManager::new(prns, config.channel_admission.clone());
        let acquisition_scheduler = AcquisitionScheduler::new(config.acquisition.clone());
        let acquisition_ring = AcquisitionRingBuffer::new(config.acquisition_ring_samples(), 0);
        let next_pvt_output_ms = config.pvt_output_rate_ms.max(1);
        Self {
            config,
            channels,
            acquisition_scheduler,
            acquisition_ring,
            active_tracking: BTreeMap::new(),
            next_pvt_output_ms,
        }
    }

    pub fn config(&self) -> &StreamingPvtConfig {
        &self.config
    }

    pub fn channels(&self) -> &ChannelManager {
        &self.channels
    }

    pub fn channels_mut(&mut self) -> &mut ChannelManager {
        &mut self.channels
    }

    pub fn state_snapshot(&self) -> StreamingPvtStateSnapshot {
        StreamingPvtStateSnapshot {
            sample_index: self.acquisition_ring.next_sample_index(),
            next_pvt_output_ms: self.next_pvt_output_ms,
            pvt_output_rate_ms: self.config.pvt_output_rate_ms,
            channels: self.channels.snapshots(&self.active_tracking_prns()),
        }
    }

    pub fn active_tracking_prns(&self) -> Vec<u8> {
        self.active_tracking.keys().copied().collect()
    }

    pub fn ingest_samples(
        &mut self,
        samples: &[Complex<f64>],
        now_ms: u64,
    ) -> Vec<StreamingPvtEvent> {
        self.acquisition_ring.push_samples(samples);
        let mut events = Vec::new();
        if let Some(scan) = self.plan_acquisition_scan(now_ms) {
            events.push(StreamingPvtEvent::AcquisitionScanPlanned(scan.clone()));
            if let Some(result) = self.execute_acquisition_scan(&scan, now_ms) {
                events.push(StreamingPvtEvent::AcquisitionScanCompleted(result));
            }
        }
        events
    }

    pub fn plan_acquisition_scan(&mut self, now_ms: u64) -> Option<StreamingAcquisitionScan> {
        let sample_count = self.config.acquisition_window_samples();
        if self.acquisition_ring.len() < sample_count {
            return None;
        }
        let plan = self
            .acquisition_scheduler
            .plan_scan(now_ms, &self.channels)?;
        let (sample_index, _) = self.acquisition_ring.latest_window(sample_count)?;
        for prn in &plan.prns {
            self.channels.set_acquiring(*prn, now_ms);
        }
        Some(StreamingAcquisitionScan {
            plan,
            sample_index,
            sample_count,
        })
    }

    pub fn acquisition_window_for_scan(
        &self,
        scan: &StreamingAcquisitionScan,
    ) -> Option<(u64, Vec<Complex<f64>>)> {
        let (sample_index, window) = self.acquisition_ring.latest_window(scan.sample_count)?;
        (sample_index == scan.sample_index).then_some((sample_index, window))
    }

    pub fn execute_acquisition_scan(
        &mut self,
        scan: &StreamingAcquisitionScan,
        now_ms: u64,
    ) -> Option<StreamingAcquisitionScanResult> {
        let (sample_index, window) = self.acquisition_window_for_scan(scan)?;
        let mut acquisition = PcpsAcquisition::new(self.pcps_config());
        let mut results = Vec::with_capacity(scan.plan.prns.len());

        for &prn in &scan.plan.prns {
            let result = acquisition.acquire_noncoherent_at_sample(
                (prn - 1) as usize,
                &window,
                sample_index,
                scan.plan.noncoherent_integrations,
            );
            if result.detected {
                self.accept_acquisition(result.clone(), now_ms);
            } else {
                self.reject_acquisition(prn, now_ms);
            }
            results.push(result);
        }

        Some(StreamingAcquisitionScanResult {
            scan: scan.clone(),
            results,
        })
    }

    fn pcps_config(&self) -> PcpsConfig {
        PcpsConfig {
            fs: self.config.sample_rate_hz,
            if_freq: self.config.intermediate_frequency_hz,
            doppler_max: self.config.acquisition.doppler_max_hz,
            doppler_step: self.config.acquisition.doppler_step_hz,
            noncoherent_integrations: self.config.acquisition.noncoherent_integrations,
            threshold: self.config.acquisition.threshold,
            ..Default::default()
        }
    }

    fn tracking_config(&self) -> TrackingConfig {
        TrackingConfig {
            fs: self.config.sample_rate_hz,
            if_freq: self.config.intermediate_frequency_hz,
            ..self.config.tracking.clone()
        }
    }

    pub fn accept_acquisition(&mut self, result: AcquisitionResult, now_ms: u64) {
        if result.detected {
            let tracker = DllPllTracking::new(
                (result.prn - 1) as usize,
                result.doppler_hz,
                result.code_phase_chips,
                self.tracking_config(),
                result.sample_index,
            );
            self.active_tracking.insert(result.prn, tracker);
        }
        self.channels.accept_acquisition(result, now_ms);
    }

    pub fn reject_acquisition(&mut self, prn: u8, now_ms: u64) {
        self.active_tracking.remove(&prn);
        self.channels.reject_acquisition(prn, now_ms);
    }

    pub fn observe_tracking(
        &mut self,
        prn: u8,
        observation: ChannelHealthObservation,
        now_ms: u64,
    ) -> ReceiverChannelState {
        self.channels.observe_tracking(prn, observation, now_ms)
    }

    pub fn process_tracking_ms(
        &mut self,
        samples: &[Complex<f64>],
        now_ms: u64,
        nav_time_available: impl Fn(u8) -> bool,
        ephemeris_available: impl Fn(u8) -> bool,
    ) -> Vec<StreamingPvtEvent> {
        let prns = self.active_tracking_prns();
        let mut events = Vec::with_capacity(prns.len());
        let mut lost_prns = Vec::new();

        for prn in prns {
            let output = self
                .active_tracking
                .get_mut(&prn)
                .expect("active PRN should have a tracker")
                .process(samples);
            let observation = ChannelHealthObservation::from_tracking_output(
                &output,
                nav_time_available(prn),
                ephemeris_available(prn),
            );
            let state = self.channels.observe_tracking(prn, observation, now_ms);
            if state == ReceiverChannelState::Lost {
                lost_prns.push(prn);
            }
            events.push(StreamingPvtEvent::TrackingUpdated(
                StreamingTrackingUpdate {
                    prn,
                    now_ms,
                    output,
                    state,
                },
            ));
        }

        for prn in lost_prns {
            self.active_tracking.remove(&prn);
        }

        events
    }

    pub fn maybe_emit_pvt(
        &mut self,
        now_ms: u64,
        output: Option<PvtEpochOutput>,
    ) -> Option<StreamingPvtEvent> {
        if now_ms < self.next_pvt_output_ms {
            return None;
        }
        while now_ms >= self.next_pvt_output_ms {
            self.next_pvt_output_ms += self.config.pvt_output_rate_ms.max(1);
        }
        output.map(|output| StreamingPvtEvent::PvtOutput(Box::new(output)))
    }

    pub fn stream_iq_file_until_ms<F>(
        &mut self,
        max_ms: usize,
        mut on_step: F,
    ) -> Result<StreamingPvtRunSummary, StreamingPvtError>
    where
        F: FnMut(StreamingPvtReadStep),
    {
        let path = self
            .config
            .iq_path
            .clone()
            .ok_or(StreamingPvtError::MissingIqPath)?;
        let mut reader = IqReader::open(path, self.config.iq_format)?;
        self.stream_iq_reader_until_ms(&mut reader, max_ms, &mut on_step)
    }

    pub fn stream_iq_reader_until_ms<F>(
        &mut self,
        reader: &mut IqReader,
        max_ms: usize,
        mut on_step: F,
    ) -> Result<StreamingPvtRunSummary, StreamingPvtError>
    where
        F: FnMut(StreamingPvtReadStep),
    {
        let max_samples = max_ms * self.config.samples_per_ms();
        let chunk_samples = self.config.chunk_samples().max(1);
        let mut summary = StreamingPvtRunSummary::default();
        let mut buffer = vec![Complex::new(0.0, 0.0); chunk_samples.min(max_samples.max(1))];

        while summary.samples_read < max_samples {
            let requested = chunk_samples.min(max_samples - summary.samples_read);
            if buffer.len() != requested {
                buffer.resize(requested, Complex::new(0.0, 0.0));
            }
            let start_sample_index = self.acquisition_ring.next_sample_index();
            let samples_read = reader.read(&mut buffer, requested)?;
            if samples_read == 0 {
                break;
            }

            let now_sample_index = start_sample_index + samples_read as u64;
            let now_ms = samples_to_ceil_ms(now_sample_index, self.config.samples_per_ms());
            let events = self.ingest_samples(&buffer[..samples_read], now_ms);
            summary.samples_read += samples_read;
            summary.steps += 1;
            summary.events_emitted += events.len();
            on_step(StreamingPvtReadStep {
                start_sample_index,
                samples_read,
                now_ms,
                events,
            });

            if samples_read < requested {
                break;
            }
        }

        Ok(summary)
    }
}

fn samples_to_ceil_ms(samples: u64, samples_per_ms: usize) -> u64 {
    let samples_per_ms = samples_per_ms.max(1) as u64;
    samples.div_ceil(samples_per_ms)
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
    use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
    use std::f64::consts::TAU;
    use std::fs;

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

    fn samples(count: usize) -> Vec<Complex<f64>> {
        (0..count)
            .map(|i| Complex::new(i as f64, -(i as f64)))
            .collect()
    }

    fn synthetic_signal_ms(
        prn_idx: usize,
        doppler_hz: f64,
        code_phase_chips: f64,
        fs: f64,
        amp: f64,
        samples: usize,
    ) -> Vec<Complex<f64>> {
        let samples_per_chip = fs / 1.023e6;
        let code = &PRN_CODES[prn_idx];
        (0..samples)
            .map(|sample_index| {
                let phase = TAU * doppler_hz * sample_index as f64 / fs;
                let chip_idx = ((sample_index as f64 / samples_per_chip + code_phase_chips)
                    as usize)
                    % CODE_LENGTH;
                let chip = code[chip_idx] as f64;
                Complex::new(phase.cos(), phase.sin()) * Complex::new(amp * chip, 0.0)
            })
            .collect()
    }

    fn write_i16_iq_file(sample_count: usize) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gnsscope_stream_test_{}_{}.dat",
            std::process::id(),
            sample_count
        ));
        let mut bytes = Vec::with_capacity(sample_count * 4);
        for i in 0..sample_count {
            let i_sample = i as i16;
            let q_sample = -(i as i16);
            bytes.extend_from_slice(&i_sample.to_le_bytes());
            bytes.extend_from_slice(&q_sample.to_le_bytes());
        }
        fs::write(&path, bytes).expect("write temp iq file");
        path
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
    fn streaming_config_defaults_to_one_ms_chunks_and_configurable_output_rate() {
        let config = StreamingPvtConfig::default().with_output_rate_ms(100);

        assert_eq!(config.chunk_ms, 1);
        assert_eq!(config.samples_per_ms(), 4_000);
        assert_eq!(config.chunk_samples(), 4_000);
        assert_eq!(config.pvt_output_rate_ms, 100);

        let config = config.with_chunk_ms(20);
        assert_eq!(config.chunk_samples(), 80_000);
    }

    #[test]
    fn streaming_runner_plans_fixed_prn_cold_acquisition_from_ring() {
        let mut config = StreamingPvtConfig::cttc_fixed_prns(vec![1, 11, 17]);
        config.acquisition.cold_acq_interval_ms = 100;
        config.acquisition.max_acq_prns_per_scan = 2;
        config.acquisition.acq_window_ms = 10;
        config.acquisition.threshold = f64::INFINITY;
        config.acquisition_ring_ms = 10;
        let mut runner = StreamingPvtRunner::new(config);

        let events = runner.ingest_samples(&samples(40_000), 1);

        assert_eq!(events.len(), 2);
        let StreamingPvtEvent::AcquisitionScanPlanned(scan) = &events[0] else {
            panic!("expected acquisition scan");
        };
        assert_eq!(scan.plan.kind, AcquisitionScanKind::Cold);
        assert_eq!(scan.plan.prns, vec![1, 11]);
        assert_eq!(scan.sample_index, 0);
        assert_eq!(scan.sample_count, 40_000);
        let StreamingPvtEvent::AcquisitionScanCompleted(completed) = &events[1] else {
            panic!("expected completed acquisition scan");
        };
        assert_eq!(completed.results.len(), 2);
        assert!(completed.results.iter().all(|result| !result.detected));
        assert_eq!(runner.channels().state(1), ReceiverChannelState::Absent);
        assert_eq!(runner.channels().state(17), ReceiverChannelState::Absent);
    }

    #[test]
    fn streaming_runner_prioritizes_lost_channel_reacquisition() {
        let mut config = StreamingPvtConfig::cttc_fixed_prns(vec![1, 11, 17]);
        config.acquisition.cold_acq_interval_ms = 100;
        config.acquisition.reacq_interval_ms = 1;
        config.channel_admission.probation_good_epochs = 1;
        config.channel_admission.lost_bad_epochs = 1;
        let mut runner = StreamingPvtRunner::new(config);
        runner.ingest_samples(&samples(40_000), 1);

        runner.accept_acquisition(acq(11, 5_500.0), 2);
        runner.observe_tracking(11, good_obs(5_500.0), 3);
        let mut bad = good_obs(5_500.0);
        bad.pll_lock_metric = 0.0;
        runner.observe_tracking(11, bad, 4);

        let scan = runner
            .plan_acquisition_scan(5)
            .expect("lost channel should be scanned first");

        assert_eq!(scan.plan.kind, AcquisitionScanKind::Reacquisition);
        assert_eq!(scan.plan.prns, vec![11]);
        assert_eq!(
            runner.channels().state(11),
            ReceiverChannelState::Reacquiring
        );
    }

    #[test]
    fn streaming_runner_pvt_output_rate_is_deterministic() {
        let config = StreamingPvtConfig::default().with_output_rate_ms(100);
        let mut runner = StreamingPvtRunner::new(config);

        assert!(runner.maybe_emit_pvt(99, None).is_none());
        assert!(runner.maybe_emit_pvt(100, None).is_none());
        assert_eq!(runner.state_snapshot().next_pvt_output_ms, 200);
        assert!(runner.maybe_emit_pvt(350, None).is_none());
        assert_eq!(runner.state_snapshot().next_pvt_output_ms, 400);
    }

    #[test]
    fn streaming_runner_reads_iq_file_in_configured_chunks() {
        let path = write_i16_iq_file(20);
        let mut config = StreamingPvtConfig::cttc_fixed_prns(vec![1, 11]);
        config.iq_path = Some(path.clone());
        config.sample_rate_hz = 1_000.0;
        config.chunk_ms = 5;
        config.acquisition.acq_window_ms = 10;
        config.acquisition.cold_acq_interval_ms = 100;
        config.acquisition.max_acq_prns_per_scan = 2;
        config.acquisition.threshold = f64::INFINITY;
        config.acquisition_ring_ms = 10;
        let mut runner = StreamingPvtRunner::new(config);
        let mut steps = Vec::new();

        let summary = runner
            .stream_iq_file_until_ms(20, |step| steps.push(step))
            .expect("stream temp iq file");
        fs::remove_file(path).ok();

        assert_eq!(summary.samples_read, 20);
        assert_eq!(summary.steps, 4);
        assert_eq!(summary.events_emitted, 2);
        assert_eq!(
            steps
                .iter()
                .map(|step| step.start_sample_index)
                .collect::<Vec<_>>(),
            vec![0, 5, 10, 15]
        );

        let scans: Vec<&StreamingAcquisitionScan> = steps
            .iter()
            .flat_map(|step| step.events.iter())
            .filter_map(|event| match event {
                StreamingPvtEvent::AcquisitionScanPlanned(scan) => Some(scan),
                StreamingPvtEvent::AcquisitionScanCompleted(_) => None,
                StreamingPvtEvent::TrackingUpdated(_) => None,
                StreamingPvtEvent::PvtOutput(_) => None,
            })
            .collect();

        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].sample_index, 0);
        assert_eq!(scans[0].sample_count, 10);
        assert_eq!(scans[0].plan.prns, vec![1, 11]);
        let completions: Vec<&StreamingAcquisitionScanResult> = steps
            .iter()
            .flat_map(|step| step.events.iter())
            .filter_map(|event| match event {
                StreamingPvtEvent::AcquisitionScanCompleted(result) => Some(result),
                StreamingPvtEvent::AcquisitionScanPlanned(_)
                | StreamingPvtEvent::TrackingUpdated(_)
                | StreamingPvtEvent::PvtOutput(_) => None,
            })
            .collect();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].results.len(), 2);
        assert_eq!(runner.state_snapshot().sample_index, 20);
    }

    #[test]
    fn streaming_runner_executes_pcps_scan_and_accepts_detection() {
        let fs = 4_000_000.0;
        let prn_idx = 5;
        let prn = (prn_idx + 1) as u8;
        let doppler = 1_500.0;
        let code_phase = 512.0;
        let mut config = StreamingPvtConfig::cttc_fixed_prns(vec![prn]);
        config.sample_rate_hz = fs;
        config.acquisition.acq_window_ms = 1;
        config.acquisition.noncoherent_integrations = 1;
        config.acquisition.cold_acq_interval_ms = 1;
        config.acquisition.doppler_max_hz = 2_000.0;
        config.acquisition.doppler_step_hz = 500.0;
        config.acquisition.threshold = 4.0;
        config.acquisition_ring_ms = 1;
        let mut runner = StreamingPvtRunner::new(config);

        let events = runner.ingest_samples(
            &synthetic_signal_ms(prn_idx, doppler, code_phase, fs, 10.0, fs as usize / 1000),
            1,
        );

        let completed = events
            .iter()
            .find_map(|event| match event {
                StreamingPvtEvent::AcquisitionScanCompleted(result) => Some(result),
                StreamingPvtEvent::AcquisitionScanPlanned(_)
                | StreamingPvtEvent::TrackingUpdated(_)
                | StreamingPvtEvent::PvtOutput(_) => None,
            })
            .expect("completed acquisition scan");
        assert_eq!(completed.results.len(), 1);
        let result = &completed.results[0];
        assert!(result.detected, "peak/mean={:.2}", result.peak_to_mean);
        assert_eq!(result.prn, prn);
        assert!((result.doppler_hz - doppler).abs() <= 500.0);
        assert!((result.code_phase_chips - code_phase).abs() < 2.0);
        assert_eq!(
            runner.channels().state(prn),
            ReceiverChannelState::TrackingProbation
        );
        assert_eq!(runner.active_tracking_prns(), vec![prn]);
        let snapshot = runner.state_snapshot();
        let channel = snapshot
            .channels
            .iter()
            .find(|channel| channel.prn == prn)
            .expect("PRN snapshot");
        assert!(channel.tracking_active);
    }

    #[test]
    fn streaming_runner_processes_active_tracking_and_promotes_channel() {
        let fs = 4_000_000.0;
        let prn_idx = 5;
        let prn = (prn_idx + 1) as u8;
        let doppler = 1_500.0;
        let code_phase = 512.0;
        let samples =
            synthetic_signal_ms(prn_idx, doppler, code_phase, fs, 10.0, fs as usize / 1000);
        let mut config = StreamingPvtConfig::cttc_fixed_prns(vec![prn]);
        config.sample_rate_hz = fs;
        config.acquisition.acq_window_ms = 1;
        config.acquisition.noncoherent_integrations = 1;
        config.acquisition.cold_acq_interval_ms = 1;
        config.acquisition.doppler_max_hz = 2_000.0;
        config.acquisition.doppler_step_hz = 500.0;
        config.acquisition.threshold = 4.0;
        config.acquisition_ring_ms = 1;
        config.channel_admission.probation_good_epochs = 1;
        config.channel_admission.min_cn0_db_hz = 0.0;
        let mut runner = StreamingPvtRunner::new(config);
        runner.ingest_samples(&samples, 1);

        let events = runner.process_tracking_ms(&samples, 2, |_| true, |_| true);

        assert_eq!(events.len(), 1);
        let StreamingPvtEvent::TrackingUpdated(update) = &events[0] else {
            panic!("expected tracking update");
        };
        assert_eq!(update.prn, prn);
        assert_eq!(update.now_ms, 2);
        assert!(update.output.pll_lock_metric > 0.0);
        assert_eq!(update.state, ReceiverChannelState::Healthy);
        assert_eq!(runner.channels().state(prn), ReceiverChannelState::Healthy);
        assert_eq!(runner.channels().pvt_eligible_prns(), vec![prn]);
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
