use crate::{GuiReceiverWorkspaceState, GuiSettingsIssue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiSettingControlKind {
    FilePicker,
    NumberInput,
    Select,
    Toggle,
    Action,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiSettingControl {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: GuiSettingControlKind,
    pub value: String,
    pub enabled: bool,
    pub issue: Option<GuiSettingsIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiMetric {
    pub id: &'static str,
    pub label: &'static str,
    pub value: String,
    pub unit: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiTable {
    pub id: &'static str,
    pub columns: Vec<&'static str>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuiScreenSection {
    pub id: &'static str,
    pub title: &'static str,
    pub metrics: Vec<GuiMetric>,
    pub controls: Vec<GuiSettingControl>,
    pub table: Option<GuiTable>,
}

pub fn build_receiver_workspace_screen(state: &GuiReceiverWorkspaceState) -> Vec<GuiScreenSection> {
    vec![
        settings_section(state),
        position_section(state),
        velocity_section(state),
        time_section(state),
        quality_section(state),
        channel_section(state),
    ]
}

fn settings_section(state: &GuiReceiverWorkspaceState) -> GuiScreenSection {
    let controls = vec![
        control(
            "iq_file_path",
            "IQ File",
            GuiSettingControlKind::FilePicker,
            state
                .settings
                .iq_file_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            true,
            issue_for(state, "iq_file_path"),
        ),
        control(
            "sample_rate_hz",
            "Sample Rate",
            GuiSettingControlKind::NumberInput,
            format!("{:.0}", state.settings.sample_rate_hz),
            true,
            issue_for(state, "sample_rate_hz"),
        ),
        control(
            "intermediate_frequency_hz",
            "Intermediate Frequency",
            GuiSettingControlKind::NumberInput,
            format!("{:.0}", state.settings.intermediate_frequency_hz),
            true,
            issue_for(state, "intermediate_frequency_hz"),
        ),
        control(
            "iq_format",
            "IQ Format",
            GuiSettingControlKind::Select,
            format!("{:?}", state.settings.iq_format),
            true,
            None,
        ),
        control(
            "rinex_nav_path",
            "RINEX Nav",
            GuiSettingControlKind::FilePicker,
            state
                .settings
                .rinex_nav_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "optional".to_string()),
            true,
            issue_for(state, "rinex_nav_path"),
        ),
        control(
            "use_rinex_nav_environment",
            "Nav Env Override",
            GuiSettingControlKind::Toggle,
            state.settings.use_rinex_nav_environment.to_string(),
            true,
            None,
        ),
        control(
            "auto_discover_colocated_nav",
            "Auto Discover Nav",
            GuiSettingControlKind::Toggle,
            state.settings.auto_discover_colocated_nav.to_string(),
            true,
            None,
        ),
        control(
            "enable_carrier_smoothing",
            "Carrier Smoothing",
            GuiSettingControlKind::Toggle,
            state.settings.enable_carrier_smoothing.to_string(),
            true,
            None,
        ),
        control(
            "enable_kalman",
            "Kalman",
            GuiSettingControlKind::Toggle,
            state.settings.enable_kalman.to_string(),
            true,
            None,
        ),
        control(
            "enable_raim",
            "RAIM",
            GuiSettingControlKind::Toggle,
            state.settings.enable_raim.to_string(),
            true,
            None,
        ),
        control(
            "start_processing",
            "Start",
            GuiSettingControlKind::Action,
            format!("{:?}", state.status),
            state.settings_issues.is_empty(),
            None,
        ),
    ];
    GuiScreenSection {
        id: "settings",
        title: "Settings",
        metrics: vec![
            metric("status", "Status", format!("{:?}", state.status), ""),
            metric("nav_source", "Nav Source", &state.nav_source.label, ""),
            metric(
                "iono",
                "Iono",
                state.nav_source.ionosphere_available.to_string(),
                "",
            ),
            metric(
                "ephemeris",
                "Ephemeris",
                state.nav_source.ephemeris_available.to_string(),
                "",
            ),
            metric(
                "progress",
                "Progress",
                format!("{:.0}", state.progress_fraction * 100.0),
                "%",
            ),
        ],
        controls,
        table: None,
    }
}

fn position_section(state: &GuiReceiverWorkspaceState) -> GuiScreenSection {
    let metrics = state
        .latest_pvt
        .as_ref()
        .map(|pvt| {
            vec![
                metric(
                    "solution_kind",
                    "Solution",
                    format!("{:?}", pvt.solution_kind),
                    "",
                ),
                metric(
                    "latitude",
                    "Latitude",
                    format!("{:.6}", pvt.latitude_deg),
                    "deg",
                ),
                metric(
                    "longitude",
                    "Longitude",
                    format!("{:.6}", pvt.longitude_deg),
                    "deg",
                ),
                metric(
                    "altitude",
                    "Altitude",
                    format!("{:.1}", pvt.altitude_m),
                    "m",
                ),
                metric("ecef_x", "ECEF X", format!("{:.1}", pvt.ecef_m[0]), "m"),
                metric("ecef_y", "ECEF Y", format!("{:.1}", pvt.ecef_m[1]), "m"),
                metric("ecef_z", "ECEF Z", format!("{:.1}", pvt.ecef_m[2]), "m"),
            ]
        })
        .unwrap_or_else(|| vec![metric("position", "Position", "unavailable", "")]);
    GuiScreenSection {
        id: "position",
        title: "Position",
        metrics,
        controls: Vec::new(),
        table: None,
    }
}

fn velocity_section(state: &GuiReceiverWorkspaceState) -> GuiScreenSection {
    let metrics = state
        .latest_pvt
        .as_ref()
        .and_then(|pvt| pvt.velocity.as_ref())
        .map(|velocity| {
            vec![
                metric(
                    "speed",
                    "Speed",
                    format!("{:.2}", velocity.speed_mps),
                    "m/s",
                ),
                metric(
                    "horizontal_speed",
                    "Horizontal",
                    format!("{:.2}", velocity.horizontal_speed_mps),
                    "m/s",
                ),
                metric("east", "East", format!("{:+.2}", velocity.east_mps), "m/s"),
                metric(
                    "north",
                    "North",
                    format!("{:+.2}", velocity.north_mps),
                    "m/s",
                ),
                metric("up", "Up", format!("{:+.2}", velocity.up_mps), "m/s"),
                metric(
                    "heading",
                    "Heading",
                    velocity
                        .heading_deg
                        .map(|heading| format!("{:.1}", heading))
                        .unwrap_or_else(|| "n/a".to_string()),
                    "deg",
                ),
                metric(
                    "clock_drift_mps",
                    "Clock Drift",
                    format!("{:+.3}", velocity.clock_drift_mps),
                    "m/s",
                ),
            ]
        })
        .unwrap_or_else(|| vec![metric("velocity", "Velocity", "unavailable", "")]);
    GuiScreenSection {
        id: "velocity",
        title: "Velocity",
        metrics,
        controls: Vec::new(),
        table: None,
    }
}

fn time_section(state: &GuiReceiverWorkspaceState) -> GuiScreenSection {
    let metrics = state
        .latest_pvt
        .as_ref()
        .map(|pvt| {
            vec![
                metric(
                    "gps_week",
                    "GPS Week",
                    pvt.time
                        .gps_week
                        .map(|week| week.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    "",
                ),
                metric(
                    "receiver_sow",
                    "Receiver SOW",
                    format_optional_f64(pvt.time.receiver_time_sow, 3),
                    "s",
                ),
                metric(
                    "corrected_sow",
                    "Corrected SOW",
                    format_optional_f64(pvt.time.corrected_gps_time_sow, 3),
                    "s",
                ),
                metric(
                    "clock_bias",
                    "Clock Bias",
                    format!("{:+.3}", pvt.time.receiver_clock_bias_us),
                    "us",
                ),
                metric(
                    "clock_drift",
                    "Clock Drift",
                    format_optional_signed_f64(pvt.time.receiver_clock_drift_ppm, 3),
                    "ppm",
                ),
            ]
        })
        .unwrap_or_else(|| vec![metric("time", "Time", "unavailable", "")]);
    GuiScreenSection {
        id: "time",
        title: "Time",
        metrics,
        controls: Vec::new(),
        table: None,
    }
}

fn quality_section(state: &GuiReceiverWorkspaceState) -> GuiScreenSection {
    let metrics = state
        .latest_pvt
        .as_ref()
        .map(|pvt| {
            vec![
                metric(
                    "converged",
                    "Converged",
                    pvt.quality.converged.to_string(),
                    "",
                ),
                metric(
                    "satellites",
                    "Satellites",
                    pvt.quality.n_sats.to_string(),
                    "",
                ),
                metric("gdop", "GDOP", format!("{:.2}", pvt.quality.gdop), ""),
                metric("hdop", "HDOP", format!("{:.2}", pvt.quality.hdop), ""),
                metric(
                    "residual_rms",
                    "Residual RMS",
                    format!("{:.2}", pvt.quality.residual_rms_m),
                    "m",
                ),
                metric(
                    "horizontal_sigma",
                    "Horizontal Sigma",
                    format!("{:.1}", pvt.quality.horizontal_sigma_m),
                    "m",
                ),
                metric(
                    "vertical_sigma",
                    "Vertical Sigma",
                    format!("{:.1}", pvt.quality.vertical_sigma_m),
                    "m",
                ),
            ]
        })
        .unwrap_or_else(|| vec![metric("quality", "Quality", "unavailable", "")]);
    GuiScreenSection {
        id: "quality",
        title: "Quality",
        metrics,
        controls: Vec::new(),
        table: None,
    }
}

fn channel_section(state: &GuiReceiverWorkspaceState) -> GuiScreenSection {
    let rows = state
        .channels
        .iter()
        .map(|channel| {
            vec![
                format!("{:02}", channel.prn),
                channel.acquired.to_string(),
                channel.tracking_locked.to_string(),
                channel.nav_locked.to_string(),
                format!("{:.1}", channel.cn0_db_hz),
                format!("{:+.0}", channel.doppler_hz),
                channel
                    .residual_m
                    .map(|residual| format!("{:+.3}", residual))
                    .unwrap_or_else(|| "n/a".to_string()),
            ]
        })
        .collect();
    GuiScreenSection {
        id: "channels",
        title: "Channel Diagnostics",
        metrics: vec![
            metric(
                "locked",
                "Locked",
                state.locked_channel_count().to_string(),
                "",
            ),
            metric("total", "Total", state.channels.len().to_string(), ""),
        ],
        controls: Vec::new(),
        table: Some(GuiTable {
            id: "channel_diagnostics",
            columns: vec![
                "PRN", "Acq", "Tracking", "Nav", "C/N0", "Doppler", "Residual",
            ],
            rows,
        }),
    }
}

fn control(
    id: &'static str,
    label: &'static str,
    kind: GuiSettingControlKind,
    value: String,
    enabled: bool,
    issue: Option<GuiSettingsIssue>,
) -> GuiSettingControl {
    GuiSettingControl {
        id,
        label,
        kind,
        value,
        enabled,
        issue,
    }
}

fn metric(
    id: &'static str,
    label: &'static str,
    value: impl Into<String>,
    unit: &'static str,
) -> GuiMetric {
    GuiMetric {
        id,
        label,
        value: value.into(),
        unit,
    }
}

fn issue_for(state: &GuiReceiverWorkspaceState, field: &'static str) -> Option<GuiSettingsIssue> {
    state
        .settings_issues
        .iter()
        .find(|issue| issue.field == field)
        .cloned()
}

fn format_optional_f64(value: Option<f64>, digits: usize) -> String {
    value
        .map(|value| format!("{value:.digits$}"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_signed_f64(value: Option<f64>, digits: usize) -> String {
    value
        .map(|value| format!("{value:+.digits$}"))
        .unwrap_or_else(|| "n/a".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GuiProcessingSettings, GuiReceiverWorkspaceState};
    use std::path::PathBuf;

    #[test]
    fn test_screen_contains_first_screen_sections() {
        let sections = build_receiver_workspace_screen(&GuiReceiverWorkspaceState::preview());
        let ids: Vec<&str> = sections.iter().map(|section| section.id).collect();

        assert_eq!(
            ids,
            vec!["settings", "position", "velocity", "time", "quality", "channels"]
        );
    }

    #[test]
    fn test_settings_controls_are_typed_for_gui_widgets() {
        let sections = build_receiver_workspace_screen(&GuiReceiverWorkspaceState::preview());
        let settings = sections
            .iter()
            .find(|section| section.id == "settings")
            .unwrap();

        assert!(settings.controls.iter().any(|control| {
            control.id == "iq_file_path" && control.kind == GuiSettingControlKind::FilePicker
        }));
        assert!(settings.controls.iter().any(|control| {
            control.id == "sample_rate_hz" && control.kind == GuiSettingControlKind::NumberInput
        }));
        assert!(settings.controls.iter().any(|control| {
            control.id == "iq_format" && control.kind == GuiSettingControlKind::Select
        }));
        assert!(settings.controls.iter().any(|control| {
            control.id == "enable_kalman" && control.kind == GuiSettingControlKind::Toggle
        }));
        assert!(settings.controls.iter().any(|control| {
            control.id == "enable_raim"
                && control.kind == GuiSettingControlKind::Toggle
                && control.value == "false"
        }));
        assert!(settings.controls.iter().any(|control| {
            control.id == "start_processing" && control.kind == GuiSettingControlKind::Action
        }));
    }

    #[test]
    fn test_screen_disables_start_when_settings_have_issues() {
        let sections = build_receiver_workspace_screen(&GuiReceiverWorkspaceState::preview());
        let settings = sections
            .iter()
            .find(|section| section.id == "settings")
            .unwrap();
        let start = settings
            .controls
            .iter()
            .find(|control| control.id == "start_processing")
            .unwrap();

        assert!(!start.enabled);
    }

    #[test]
    fn test_screen_enables_start_for_ready_iq_only_state() {
        let state = GuiReceiverWorkspaceState::new(GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            use_rinex_nav_environment: false,
            auto_discover_colocated_nav: false,
            ..Default::default()
        });
        let sections = build_receiver_workspace_screen(&state);
        let start = sections
            .iter()
            .find(|section| section.id == "settings")
            .unwrap()
            .controls
            .iter()
            .find(|control| control.id == "start_processing")
            .unwrap();

        assert!(start.enabled);
    }

    #[test]
    fn test_settings_section_exposes_nav_source_status() {
        let mut state = GuiReceiverWorkspaceState::preview();
        state.nav_source = crate::GuiNavSourceStatus::from_rinex_capabilities(
            gnss_core::nav::RinexNavCandidate {
                path: PathBuf::from("data/brdc0940.13n"),
                source_kind: gnss_core::nav::RinexNavSourceKind::Colocated,
                source_label: "rinex-nav-colocated".to_string(),
            },
            true,
            true,
        );
        let sections = build_receiver_workspace_screen(&state);
        let settings = sections
            .iter()
            .find(|section| section.id == "settings")
            .unwrap();

        assert!(settings
            .metrics
            .iter()
            .any(|metric| { metric.id == "nav_source" && metric.value == "Colocated RINEX nav" }));
        assert!(settings
            .metrics
            .iter()
            .any(|metric| metric.id == "iono" && metric.value == "true"));
        assert!(settings
            .metrics
            .iter()
            .any(|metric| metric.id == "ephemeris" && metric.value == "true"));
    }

    #[test]
    fn test_channel_table_columns_are_stable() {
        let sections = build_receiver_workspace_screen(&GuiReceiverWorkspaceState::preview());
        let table = sections
            .iter()
            .find(|section| section.id == "channels")
            .unwrap()
            .table
            .as_ref()
            .unwrap();

        assert_eq!(
            table.columns,
            vec!["PRN", "Acq", "Tracking", "Nav", "C/N0", "Doppler", "Residual"]
        );
        assert_eq!(table.rows.len(), 5);
    }
}
