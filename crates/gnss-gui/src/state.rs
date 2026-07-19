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
    match settings.iq_file_path.as_deref() {
        Some(path) if path.as_os_str().is_empty() => issues.push(GuiSettingsIssue {
            field: "iq_file_path",
            message: "Select an IQ file",
        }),
        Some(path) if !looks_like_local_path(path) => issues.push(GuiSettingsIssue {
            field: "iq_file_path",
            message: "IQ file path is invalid",
        }),
        None => issues.push(GuiSettingsIssue {
            field: "iq_file_path",
            message: "Select an IQ file",
        }),
        _ => {}
    }
    if !settings.sample_rate_hz.is_finite() || settings.sample_rate_hz <= 0.0 {
        issues.push(GuiSettingsIssue {
            field: "sample_rate_hz",
            message: "Sample rate must be positive",
        });
    }
    if !settings.intermediate_frequency_hz.is_finite() {
        issues.push(GuiSettingsIssue {
            field: "intermediate_frequency_hz",
            message: "Intermediate frequency must be finite",
        });
    }
    if settings
        .rinex_nav_path
        .as_deref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        issues.push(GuiSettingsIssue {
            field: "rinex_nav_path",
            message: "Navigation file path is invalid",
        });
    }
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
