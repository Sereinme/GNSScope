use crate::{
    GuiNavSourceStatus, GuiProcessingSettings, GuiPvtViewModel, GuiReceiverEvent,
    ReceiverWorkspaceLayout,
};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiReceiverStatus {
    Idle,
    Ready,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiChannelStatus {
    pub prn: u8,
    pub acquired: bool,
    pub tracking_locked: bool,
    pub nav_locked: bool,
    pub cn0_db_hz: f64,
    pub doppler_hz: f64,
    pub residual_m: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiSettingsIssue {
    pub field: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Clone)]
pub struct GuiReceiverWorkspaceState {
    pub settings: GuiProcessingSettings,
    pub status: GuiReceiverStatus,
    pub progress_fraction: f32,
    pub status_message: String,
    pub latest_pvt: Option<GuiPvtViewModel>,
    pub nav_source: GuiNavSourceStatus,
    pub channels: Vec<GuiChannelStatus>,
    pub settings_issues: Vec<GuiSettingsIssue>,
}

impl GuiReceiverWorkspaceState {
    pub fn new(settings: GuiProcessingSettings) -> Self {
        let settings_issues = validate_settings(&settings);
        let status = if settings_issues.is_empty() {
            GuiReceiverStatus::Ready
        } else {
            GuiReceiverStatus::Idle
        };
        Self {
            settings,
            status,
            progress_fraction: 0.0,
            status_message: "Waiting for IQ file".to_string(),
            latest_pvt: None,
            nav_source: GuiNavSourceStatus::iq_only(),
            channels: Vec::new(),
            settings_issues,
        }
    }

    pub fn preview() -> Self {
        let mut state = Self::new(GuiProcessingSettings::default());
        state.latest_pvt = Some(crate::sample_pvt_view_model());
        state.channels = sample_channel_statuses();
        state.status_message = "Preview data".to_string();
        state
    }

    pub fn update_settings(&mut self, settings: GuiProcessingSettings) {
        self.settings = settings;
        self.settings_issues = validate_settings(&self.settings);
        if self.status != GuiReceiverStatus::Processing {
            self.status = if self.settings_issues.is_empty() {
                GuiReceiverStatus::Ready
            } else {
                GuiReceiverStatus::Idle
            };
        }
    }

    pub fn start_processing(&mut self) -> bool {
        self.settings_issues = validate_settings(&self.settings);
        if !self.settings_issues.is_empty() {
            self.status = GuiReceiverStatus::Idle;
            self.status_message = "Fix settings before processing".to_string();
            return false;
        }
        self.status = GuiReceiverStatus::Processing;
        self.progress_fraction = 0.0;
        self.status_message = "Processing IQ file".to_string();
        true
    }

    pub fn update_progress(&mut self, progress_fraction: f32, message: impl Into<String>) {
        self.progress_fraction = progress_fraction.clamp(0.0, 1.0);
        self.status_message = message.into();
    }

    pub fn update_channel(&mut self, channel: GuiChannelStatus) {
        match self
            .channels
            .iter_mut()
            .find(|status| status.prn == channel.prn)
        {
            Some(existing) => *existing = channel,
            None => self.channels.push(channel),
        }
        self.channels.sort_by_key(|channel| channel.prn);
    }

    pub fn update_pvt(&mut self, latest_pvt: GuiPvtViewModel) {
        self.latest_pvt = Some(latest_pvt);
    }

    pub fn update_nav_source(&mut self, nav_source: GuiNavSourceStatus) {
        self.nav_source = nav_source;
    }

    pub fn complete(&mut self, latest_pvt: GuiPvtViewModel, channels: Vec<GuiChannelStatus>) {
        self.latest_pvt = Some(latest_pvt);
        self.channels = channels;
        self.progress_fraction = 1.0;
        self.status = GuiReceiverStatus::Completed;
        self.status_message = "Processing complete".to_string();
    }

    pub fn finish_processing(&mut self) {
        self.progress_fraction = 1.0;
        self.status = GuiReceiverStatus::Completed;
        self.status_message = "Processing complete".to_string();
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.status = GuiReceiverStatus::Failed;
        self.status_message = message.into();
    }

    pub fn apply_event(&mut self, event: GuiReceiverEvent) -> bool {
        match event {
            GuiReceiverEvent::SettingsChanged(settings) => {
                self.update_settings(*settings);
                true
            }
            GuiReceiverEvent::ProcessingStarted => self.start_processing(),
            GuiReceiverEvent::Progress { fraction, message } => {
                self.update_progress(fraction, message);
                true
            }
            GuiReceiverEvent::NavSourceUpdated(nav_source) => {
                self.update_nav_source(nav_source);
                true
            }
            GuiReceiverEvent::ChannelUpdated(channel) => {
                self.update_channel(channel);
                true
            }
            GuiReceiverEvent::PvtUpdated(pvt) => {
                self.update_pvt(*pvt);
                true
            }
            GuiReceiverEvent::ProcessingCompleted => {
                self.finish_processing();
                true
            }
            GuiReceiverEvent::ProcessingFailed(message) => {
                self.fail(message);
                true
            }
        }
    }

    pub fn layout(&self) -> ReceiverWorkspaceLayout {
        ReceiverWorkspaceLayout::new(self.settings.clone(), self.latest_pvt.clone())
    }

    pub fn locked_channel_count(&self) -> usize {
        self.channels
            .iter()
            .filter(|channel| channel.tracking_locked)
            .count()
    }
}

pub fn validate_settings(settings: &GuiProcessingSettings) -> Vec<GuiSettingsIssue> {
    let mut issues = Vec::new();
    let mut issue = |field, message| issues.push(GuiSettingsIssue { field, message });
    match settings.iq_file_path.as_deref() {
        Some(path) if path.as_os_str().is_empty() => issue("iq_file_path", "Select an IQ file"),
        Some(path) if !looks_like_local_path(path) => {
            issue("iq_file_path", "IQ file path is invalid")
        }
        None => issue("iq_file_path", "Select an IQ file"),
        _ => {}
    }
    if !settings.sample_rate_hz.is_finite() || settings.sample_rate_hz <= 0.0 {
        issue("sample_rate_hz", "Sample rate must be positive");
    }
    if !settings.intermediate_frequency_hz.is_finite() {
        issue(
            "intermediate_frequency_hz",
            "Intermediate frequency must be finite",
        );
    } else if settings.sample_rate_hz.is_finite()
        && settings.sample_rate_hz > 0.0
        && settings.intermediate_frequency_hz.abs() > settings.sample_rate_hz / 2.0
    {
        issue(
            "intermediate_frequency_hz",
            "Intermediate frequency must be within Nyquist",
        );
    }
    if settings
        .rinex_nav_path
        .as_deref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        issue("rinex_nav_path", "Navigation file path is invalid");
    }
    if settings.chunk_ms == 0 || settings.chunk_ms > 1_000 {
        issue("chunk_ms", "Chunk duration must be 1-1000 ms");
    }
    if settings.pvt_output_rate_ms == 0 || settings.pvt_output_rate_ms > 60_000 {
        issue("pvt_output_rate_ms", "PVT epoch must be 1-60000 ms");
    } else if settings.chunk_ms > 0 && settings.pvt_output_rate_ms < settings.chunk_ms as u64 {
        issue(
            "pvt_output_rate_ms",
            "PVT epoch cannot be shorter than the input chunk",
        );
    }
    if settings.acquisition_window_ms == 0 || settings.acquisition_window_ms > 1_000 {
        issue(
            "acquisition_window_ms",
            "Acquisition window must be 1-1000 ms",
        );
    }
    if settings.acquisition_noncoherent_integrations == 0
        || settings.acquisition_noncoherent_integrations > 1_000
    {
        issue(
            "acquisition_noncoherent_integrations",
            "Noncoherent integrations must be 1-1000",
        );
    }
    if !settings.acquisition_threshold.is_finite() || settings.acquisition_threshold <= 0.0 {
        issue(
            "acquisition_threshold",
            "Acquisition threshold must be positive",
        );
    }
    if settings.cold_acquisition_interval_ms == 0 {
        issue(
            "cold_acquisition_interval_ms",
            "Cold acquisition interval must be nonzero",
        );
    }
    if settings.reacquisition_interval_ms == 0 {
        issue(
            "reacquisition_interval_ms",
            "Reacquisition interval must be nonzero",
        );
    }
    if !settings.acquisition_doppler_max_hz.is_finite()
        || settings.acquisition_doppler_max_hz <= 0.0
    {
        issue(
            "acquisition_doppler_max_hz",
            "Maximum Doppler must be positive",
        );
    }
    if !settings.acquisition_doppler_step_hz.is_finite()
        || settings.acquisition_doppler_step_hz <= 0.0
        || (settings.acquisition_doppler_max_hz.is_finite()
            && settings.acquisition_doppler_step_hz > settings.acquisition_doppler_max_hz)
    {
        issue(
            "acquisition_doppler_step_hz",
            "Doppler step must be positive and no larger than the search span",
        );
    }
    if !(1..=32).contains(&settings.max_acquisition_prns_per_scan) {
        issue(
            "max_acquisition_prns_per_scan",
            "PRNs per scan must be 1-32",
        );
    }
    if !settings.dll_bandwidth_hz.is_finite() || settings.dll_bandwidth_hz <= 0.0 {
        issue("dll_bandwidth_hz", "DLL bandwidth must be positive");
    }
    if !settings.pll_bandwidth_hz.is_finite() || settings.pll_bandwidth_hz <= 0.0 {
        issue("pll_bandwidth_hz", "PLL bandwidth must be positive");
    } else if settings.dll_bandwidth_hz.is_finite()
        && settings.dll_bandwidth_hz > settings.pll_bandwidth_hz
    {
        issue(
            "dll_bandwidth_hz",
            "DLL bandwidth cannot exceed PLL bandwidth",
        );
    }
    if !settings.early_late_spacing_chips.is_finite()
        || !(0.0..=1.0).contains(&settings.early_late_spacing_chips)
        || settings.early_late_spacing_chips == 0.0
    {
        issue(
            "early_late_spacing_chips",
            "Early/late spacing must be within (0, 1] chips",
        );
    }
    if settings.cn0_samples == 0 || settings.cn0_samples > 10_000 {
        issue("cn0_samples", "C/N0 window must be 1-10000 samples");
    }
    if !settings.cn0_smoother_alpha.is_finite()
        || !(0.0..=1.0).contains(&settings.cn0_smoother_alpha)
    {
        issue(
            "cn0_smoother_alpha",
            "C/N0 smoothing alpha must be within 0-1",
        );
    }
    if !settings.minimum_pll_lock_metric.is_finite()
        || !(0.0..=1.0).contains(&settings.minimum_pll_lock_metric)
    {
        issue(
            "minimum_pll_lock_metric",
            "PLL lock threshold must be within 0-1",
        );
    }
    if !settings.minimum_cn0_db_hz.is_finite()
        || !(0.0..=80.0).contains(&settings.minimum_cn0_db_hz)
    {
        issue(
            "minimum_cn0_db_hz",
            "Minimum C/N0 must be within 0-80 dB-Hz",
        );
    }
    if settings.probation_good_epochs == 0 || settings.probation_good_epochs > 10_000 {
        issue(
            "probation_good_epochs",
            "Probation length must be 1-10000 epochs",
        );
    }
    if let Some(prns) = settings.fixed_prns.as_deref() {
        let mut seen = [false; 33];
        if prns.is_empty()
            || prns.iter().any(|prn| {
                !(1..=32).contains(prn) || std::mem::replace(&mut seen[*prn as usize], true)
            })
        {
            issue(
                "fixed_prns",
                "Fixed PRNs must be unique GPS PRNs in the range 1-32",
            );
        }
    }
    if let Some([latitude, longitude, altitude]) = settings.initial_position_lla_deg_m {
        if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
            issue(
                "initial_position_lla_deg_m",
                "Latitude must be within -90 to 90 degrees",
            );
        }
        if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
            issue(
                "initial_position_lla_deg_m",
                "Longitude must be within -180 to 180 degrees",
            );
        }
        if !altitude.is_finite() || !(-1_000.0..=100_000.0).contains(&altitude) {
            issue(
                "initial_position_lla_deg_m",
                "Altitude prior must be within -1000 to 100000 m",
            );
        }
    }
    drop(issue);
    issues
}

pub fn sample_channel_statuses() -> Vec<GuiChannelStatus> {
    vec![
        channel(20, 45.3, 8_269.0, Some(0.008)),
        channel(1, 48.8, 7_001.0, Some(-0.185)),
        channel(32, 45.0, 6_370.0, Some(0.022)),
        channel(11, 46.8, 5_404.0, Some(0.003)),
        channel(17, 45.1, 9_870.0, Some(-0.069)),
    ]
}

fn channel(prn: u8, cn0_db_hz: f64, doppler_hz: f64, residual_m: Option<f64>) -> GuiChannelStatus {
    GuiChannelStatus {
        prn,
        acquired: true,
        tracking_locked: true,
        nav_locked: true,
        cn0_db_hz,
        doppler_hz,
        residual_m,
    }
}

fn looks_like_local_path(path: &Path) -> bool {
    path.components().next().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_workspace_state_requires_iq_file_before_processing() {
        let mut state = GuiReceiverWorkspaceState::new(GuiProcessingSettings::default());

        assert_eq!(state.status, GuiReceiverStatus::Idle);
        assert!(!state.start_processing());
        assert!(state
            .settings_issues
            .iter()
            .any(|issue| issue.field == "iq_file_path"));
    }

    #[test]
    fn test_workspace_state_can_start_with_iq_only_settings() {
        let mut state = GuiReceiverWorkspaceState::new(GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            rinex_nav_path: None,
            ..Default::default()
        });

        assert_eq!(state.status, GuiReceiverStatus::Ready);
        assert!(state.start_processing());
        assert_eq!(state.status, GuiReceiverStatus::Processing);
    }

    #[test]
    fn test_workspace_state_complete_updates_pvt_and_channels() {
        let mut state = GuiReceiverWorkspaceState::new(GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            ..Default::default()
        });
        state.start_processing();
        state.complete(crate::sample_pvt_view_model(), sample_channel_statuses());

        assert_eq!(state.status, GuiReceiverStatus::Completed);
        assert_eq!(state.progress_fraction, 1.0);
        assert!(state.latest_pvt.is_some());
        assert_eq!(state.locked_channel_count(), 5);
    }

    #[test]
    fn test_validate_settings_rejects_bad_numbers() {
        let issues = validate_settings(&GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            sample_rate_hz: 0.0,
            intermediate_frequency_hz: f64::NAN,
            ..Default::default()
        });

        assert!(issues.iter().any(|issue| issue.field == "sample_rate_hz"));
        assert!(issues
            .iter()
            .any(|issue| issue.field == "intermediate_frequency_hz"));
    }

    #[test]
    fn test_validate_settings_covers_expert_receiver_constraints() {
        let issues = validate_settings(&GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            sample_rate_hz: 1_000_000.0,
            intermediate_frequency_hz: 600_000.0,
            pvt_output_rate_ms: 10,
            chunk_ms: 20,
            acquisition_doppler_max_hz: 100.0,
            acquisition_doppler_step_hz: 200.0,
            dll_bandwidth_hz: 20.0,
            pll_bandwidth_hz: 5.0,
            early_late_spacing_chips: 0.0,
            cn0_smoother_alpha: 2.0,
            fixed_prns: Some(vec![1, 1, 33]),
            initial_position_lla_deg_m: Some([91.0, 181.0, 200_000.0]),
            ..Default::default()
        });

        for field in [
            "intermediate_frequency_hz",
            "pvt_output_rate_ms",
            "acquisition_doppler_step_hz",
            "dll_bandwidth_hz",
            "early_late_spacing_chips",
            "cn0_smoother_alpha",
            "fixed_prns",
            "initial_position_lla_deg_m",
        ] {
            assert!(issues.iter().any(|issue| issue.field == field), "{field}");
        }
    }

    #[test]
    fn test_workspace_state_applies_receiver_events() {
        let mut state = GuiReceiverWorkspaceState::new(GuiProcessingSettings::default());

        assert!(
            state.apply_event(GuiReceiverEvent::settings_changed(GuiProcessingSettings {
                iq_file_path: Some(PathBuf::from("signal.dat")),
                ..Default::default()
            }))
        );
        assert_eq!(state.status, GuiReceiverStatus::Ready);
        assert!(state.apply_event(GuiReceiverEvent::ProcessingStarted));
        assert_eq!(state.status, GuiReceiverStatus::Processing);

        state.apply_event(GuiReceiverEvent::Progress {
            fraction: 0.42,
            message: "Tracking channels".to_string(),
        });
        assert_eq!(state.progress_fraction, 0.42);
        assert_eq!(state.status_message, "Tracking channels");

        state.apply_event(GuiReceiverEvent::NavSourceUpdated(
            GuiNavSourceStatus::from_rinex_candidate(
                gnss_core::nav::RinexNavCandidate {
                    path: PathBuf::from("data/brdc0940.13n"),
                    source_kind: gnss_core::nav::RinexNavSourceKind::Colocated,
                    source_label: "rinex-nav-colocated".to_string(),
                },
                true,
            ),
        ));
        assert_eq!(state.nav_source.label, "Colocated RINEX nav");
        assert!(state.nav_source.ionosphere_available);

        state.apply_event(GuiReceiverEvent::ChannelUpdated(GuiChannelStatus {
            prn: 20,
            acquired: true,
            tracking_locked: true,
            nav_locked: false,
            cn0_db_hz: 41.0,
            doppler_hz: 1200.0,
            residual_m: None,
        }));
        state.apply_event(GuiReceiverEvent::ChannelUpdated(GuiChannelStatus {
            prn: 20,
            acquired: true,
            tracking_locked: true,
            nav_locked: true,
            cn0_db_hz: 44.0,
            doppler_hz: 1198.0,
            residual_m: Some(0.3),
        }));
        assert_eq!(state.channels.len(), 1);
        assert!(state.channels[0].nav_locked);

        state.apply_event(GuiReceiverEvent::pvt_updated(crate::sample_pvt_view_model()));
        assert!(state.latest_pvt.is_some());
        state.apply_event(GuiReceiverEvent::ProcessingCompleted);
        assert_eq!(state.status, GuiReceiverStatus::Completed);
        assert_eq!(state.progress_fraction, 1.0);
    }

    #[test]
    fn test_workspace_state_rejects_start_event_when_settings_invalid() {
        let mut state = GuiReceiverWorkspaceState::new(GuiProcessingSettings::default());

        assert!(!state.apply_event(GuiReceiverEvent::ProcessingStarted));
        assert_eq!(state.status, GuiReceiverStatus::Idle);
        assert_eq!(state.status_message, "Fix settings before processing");
    }
}
