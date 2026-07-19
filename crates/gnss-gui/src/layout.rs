use crate::{GuiProcessingSettings, GuiPvtViewModel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiPanelKind {
    Settings,
    Position,
    Velocity,
    Time,
    Quality,
    ChannelDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiPanel {
    pub kind: GuiPanelKind,
    pub title: &'static str,
    pub column: u8,
    pub row: u8,
    pub column_span: u8,
    pub row_span: u8,
}

#[derive(Debug, Clone)]
pub struct ReceiverWorkspaceLayout {
    pub settings: GuiProcessingSettings,
    pub latest_pvt: Option<GuiPvtViewModel>,
    pub panels: Vec<GuiPanel>,
}

impl ReceiverWorkspaceLayout {
    pub fn new(settings: GuiProcessingSettings, latest_pvt: Option<GuiPvtViewModel>) -> Self {
        Self {
            settings,
            latest_pvt,
            panels: default_receiver_workspace_panels(),
        }
    }

    pub fn default_preview() -> Self {
        Self::new(
            GuiProcessingSettings::default(),
            Some(crate::sample_pvt_view_model()),
        )
    }

    pub fn panel(&self, kind: GuiPanelKind) -> Option<&GuiPanel> {
        self.panels.iter().find(|panel| panel.kind == kind)
    }

    pub fn is_process_enabled(&self) -> bool {
        self.settings.is_ready_to_process()
    }

    pub fn nav_status_label(&self) -> &'static str {
        if self.settings.rinex_nav_path.is_some() {
            "RINEX nav file selected"
        } else if self.settings.use_rinex_nav_environment
            && self.settings.auto_discover_colocated_nav
        {
            "RINEX nav env override and auto discovery enabled"
        } else if self.settings.use_rinex_nav_environment {
            "RINEX nav environment override enabled"
        } else if self.settings.auto_discover_colocated_nav {
            "RINEX nav auto discovery enabled"
        } else {
            "IQ-only mode"
        }
    }
}

fn default_receiver_workspace_panels() -> Vec<GuiPanel> {
    vec![
        GuiPanel {
            kind: GuiPanelKind::Settings,
            title: "Settings",
            column: 0,
            row: 0,
            column_span: 1,
            row_span: 4,
        },
        GuiPanel {
            kind: GuiPanelKind::Position,
            title: "Position",
            column: 1,
            row: 0,
            column_span: 2,
            row_span: 1,
        },
        GuiPanel {
            kind: GuiPanelKind::Velocity,
            title: "Velocity",
            column: 3,
            row: 0,
            column_span: 1,
            row_span: 1,
        },
        GuiPanel {
            kind: GuiPanelKind::Time,
            title: "Time",
            column: 1,
            row: 1,
            column_span: 1,
            row_span: 1,
        },
        GuiPanel {
            kind: GuiPanelKind::Quality,
            title: "Quality",
            column: 2,
            row: 1,
            column_span: 2,
            row_span: 1,
        },
        GuiPanel {
            kind: GuiPanelKind::ChannelDiagnostics,
            title: "Channel Diagnostics",
            column: 1,
            row: 2,
            column_span: 3,
            row_span: 2,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_workspace_layout_contains_expected_first_screen_panels() {
        let layout = ReceiverWorkspaceLayout::default_preview();

        for kind in [
            GuiPanelKind::Settings,
            GuiPanelKind::Position,
            GuiPanelKind::Velocity,
            GuiPanelKind::Time,
            GuiPanelKind::Quality,
            GuiPanelKind::ChannelDiagnostics,
        ] {
            assert!(layout.panel(kind).is_some(), "missing panel: {kind:?}");
        }
    }

    #[test]
    fn test_workspace_layout_uses_iq_only_label_when_nav_disabled() {
        let layout = ReceiverWorkspaceLayout::new(
            GuiProcessingSettings {
                auto_discover_colocated_nav: false,
                use_rinex_nav_environment: false,
                ..Default::default()
            },
            None,
        );

        assert_eq!(layout.nav_status_label(), "IQ-only mode");
    }

    #[test]
    fn test_workspace_layout_reports_environment_nav_override() {
        let layout = ReceiverWorkspaceLayout::new(
            GuiProcessingSettings {
                use_rinex_nav_environment: true,
                auto_discover_colocated_nav: false,
                ..Default::default()
            },
            None,
        );

        assert_eq!(
            layout.nav_status_label(),
            "RINEX nav environment override enabled"
        );
    }

    #[test]
    fn test_workspace_layout_reports_combined_nav_policy() {
        let layout = ReceiverWorkspaceLayout::new(GuiProcessingSettings::default(), None);

        assert_eq!(
            layout.nav_status_label(),
            "RINEX nav env override and auto discovery enabled"
        );
    }

    #[test]
    fn test_workspace_layout_process_enabled_tracks_settings() {
        let mut settings = GuiProcessingSettings::default();
        let layout = ReceiverWorkspaceLayout::new(settings.clone(), None);
        assert!(!layout.is_process_enabled());

        settings.iq_file_path = Some(PathBuf::from("signal.dat"));
        let layout = ReceiverWorkspaceLayout::new(settings, None);
        assert!(layout.is_process_enabled());
    }
}
