use crate::ui::overlays::{matches_query, CommandPaletteSelect, CommandPaletteView, CommandResult};
use crate::ui::run_configuration::{
    inspector_row, inspector_section, ConfigurationEntry, ConfigurationView,
};
use crate::ui::shell::{shell_body, shell_root, workspace_shell, ShellLayout};
use crate::visual_system::{
    apply_component_theme, data_font_family, gnss_badge, gnss_divider, gnss_input, gnss_progress,
    gnss_scroll_area, gnss_search_input, interface_font_family, metrics, BadgeKind, ButtonKind,
    ContextMenuExt, ControlSize, ControlState, GnssButton, GnssMenuItem, GnssRow, GnssSwitch,
    Palette,
};
use crate::{
    spawn_streaming_worker, ActiveWorker, GuiChannelStatus, GuiDiagnosticEvent,
    GuiDiagnosticEventKind, GuiProcessingSettings, GuiPvtViewModel, GuiSatelliteGeometry,
    GuiSettingsIssue, ProcessingRun, RunLibrary, RunPersistenceWriter, RunStatus, StoredEpoch,
    WorkerEvent, WORKER_EVENT_CAPACITY,
};
use gnss_core::pvt::{PvtCandidateDisposition, PvtEpochSolutionKind};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
    progress::Progress,
    scroll::ScrollableElement,
    slider::{SliderEvent, SliderState},
    ActiveTheme, Disableable, IconName, Theme, ThemeMode, TitleBar,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    bounds, canvas, div, point, px, quad, size, AnyElement, App, Context, Entity, Focusable, Hsla,
    InteractiveElement, IntoElement, ParentElement, PathBuilder, PathPromptOptions, Render,
    SharedString, Styled, Window,
};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use uuid::Uuid;

const EDITABLE_SETTING_FIELDS: &[&str] = &[
    "sample_rate_hz",
    "intermediate_frequency_hz",
    "rinex_nav_path",
    "initial_position_lla_deg_m",
    "chunk_ms",
    "pvt_output_rate_ms",
    "acquisition_window_ms",
    "acquisition_noncoherent_integrations",
    "acquisition_threshold",
    "cold_acquisition_interval_ms",
    "acquisition_doppler_max_hz",
    "acquisition_doppler_step_hz",
    "reacquisition_interval_ms",
    "max_acquisition_prns_per_scan",
    "dll_bandwidth_hz",
    "pll_bandwidth_hz",
    "early_late_spacing_chips",
    "cn0_samples",
    "cn0_smoother_alpha",
    "minimum_pll_lock_metric",
    "minimum_cn0_db_hz",
    "probation_good_epochs",
    "fixed_prns",
];

fn setting_text_value(settings: &GuiProcessingSettings, field: &str) -> String {
    match field {
        "sample_rate_hz" => format!("{:.6}", settings.sample_rate_hz / 1e6),
        "intermediate_frequency_hz" => format!("{:.6}", settings.intermediate_frequency_hz / 1e6),
        "rinex_nav_path" => settings
            .rinex_nav_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        "initial_position_lla_deg_m" => settings
            .initial_position_lla_deg_m
            .map(|[lat, lon, alt]| format!("{lat:.7}, {lon:.7}, {alt:.3}"))
            .unwrap_or_default(),
        "fixed_prns" => settings
            .fixed_prns
            .as_ref()
            .map(|prns| prns.iter().map(u8::to_string).collect::<Vec<_>>().join(","))
            .unwrap_or_default(),
        "chunk_ms" => settings.chunk_ms.to_string(),
        "pvt_output_rate_ms" => settings.pvt_output_rate_ms.to_string(),
        "acquisition_window_ms" => settings.acquisition_window_ms.to_string(),
        "acquisition_noncoherent_integrations" => {
            settings.acquisition_noncoherent_integrations.to_string()
        }
        "acquisition_threshold" => settings.acquisition_threshold.to_string(),
        "cold_acquisition_interval_ms" => settings.cold_acquisition_interval_ms.to_string(),
        "acquisition_doppler_max_hz" => settings.acquisition_doppler_max_hz.to_string(),
        "acquisition_doppler_step_hz" => settings.acquisition_doppler_step_hz.to_string(),
        "reacquisition_interval_ms" => settings.reacquisition_interval_ms.to_string(),
        "max_acquisition_prns_per_scan" => settings.max_acquisition_prns_per_scan.to_string(),
        "dll_bandwidth_hz" => settings.dll_bandwidth_hz.to_string(),
        "pll_bandwidth_hz" => settings.pll_bandwidth_hz.to_string(),
        "early_late_spacing_chips" => settings.early_late_spacing_chips.to_string(),
        "cn0_samples" => settings.cn0_samples.to_string(),
        "cn0_smoother_alpha" => settings.cn0_smoother_alpha.to_string(),
        "minimum_pll_lock_metric" => settings.minimum_pll_lock_metric.to_string(),
        "minimum_cn0_db_hz" => settings.minimum_cn0_db_hz.to_string(),
        "probation_good_epochs" => settings.probation_good_epochs.to_string(),
        _ => String::new(),
    }
}

fn parse_fixed_prns(value: &str) -> Result<Option<Vec<u8>>, ()> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    value
        .split(|character: char| character == ',' || character == ';' || character.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u8>().map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn parse_initial_lla(value: &str) -> Result<Option<[f64; 3]>, ()> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let values = value
        .split([',', ';'])
        .map(|part| part.trim().parse::<f64>().map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()?;
    (values.len() == 3)
        .then(|| [values[0], values[1], values[2]])
        .ok_or(())
        .map(Some)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThemePreference {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InterfaceLanguage {
    SimplifiedChinese,
    English,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppPage {
    NewAnalysis,
    Workspace,
    RunLibrary,
    InterfaceSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    Overview,
    Channels,
    Position,
    Navigation,
    Candidates,
}

impl WorkspaceView {
    const ALL: [Self; 5] = [
        Self::Overview,
        Self::Channels,
        Self::Position,
        Self::Navigation,
        Self::Candidates,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Channels => "channels",
            Self::Position => "position",
            Self::Navigation => "navigation",
            Self::Candidates => "candidates",
        }
    }

    fn label(self, language: InterfaceLanguage) -> &'static str {
        match (self, language) {
            (Self::Overview, InterfaceLanguage::SimplifiedChinese) => "总览",
            (Self::Channels, InterfaceLanguage::SimplifiedChinese) => "通道",
            (Self::Position, InterfaceLanguage::SimplifiedChinese) => "定位",
            (Self::Navigation, InterfaceLanguage::SimplifiedChinese) => "导航",
            (Self::Candidates, InterfaceLanguage::SimplifiedChinese) => "候选解",
            (Self::Overview, InterfaceLanguage::English) => "Overview",
            (Self::Channels, InterfaceLanguage::English) => "Channels",
            (Self::Position, InterfaceLanguage::English) => "Position",
            (Self::Navigation, InterfaceLanguage::English) => "Navigation",
            (Self::Candidates, InterfaceLanguage::English) => "Candidates",
        }
    }

    fn description(self, language: InterfaceLanguage) -> &'static str {
        match (self, language) {
            (Self::Overview, InterfaceLanguage::SimplifiedChinese) => {
                "查看卫星几何、位置轨迹和当前 PVT 摘要"
            }
            (Self::Channels, InterfaceLanguage::SimplifiedChinese) => {
                "检查各 PRN 的锁定、C/N0、Doppler 和残差"
            }
            (Self::Position, InterfaceLanguage::SimplifiedChinese) => {
                "分析位置、速度、高度和解算质量的时间变化"
            }
            (Self::Navigation, InterfaceLanguage::SimplifiedChinese) => {
                "查看捕获、重捕获、跟踪状态和导航电文事件"
            }
            (Self::Candidates, InterfaceLanguage::SimplifiedChinese) => {
                "比较 Raw、Smoothed 与 Kalman 候选解"
            }
            (Self::Overview, InterfaceLanguage::English) => {
                "Inspect satellite geometry, position track, and current PVT"
            }
            (Self::Channels, InterfaceLanguage::English) => {
                "Inspect per-PRN lock, C/N0, Doppler, and residuals"
            }
            (Self::Position, InterfaceLanguage::English) => {
                "Analyze position, velocity, altitude, and solution quality"
            }
            (Self::Navigation, InterfaceLanguage::English) => {
                "Review acquisition, reacquisition, tracking, and NAV events"
            }
            (Self::Candidates, InterfaceLanguage::English) => {
                "Compare Raw, Smoothed, and Kalman candidates"
            }
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Overview => IconName::LayoutDashboard,
            Self::Channels => IconName::Inspector,
            Self::Position => IconName::Map,
            Self::Navigation => IconName::Globe,
            Self::Candidates => IconName::ChartPie,
        }
    }

    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("channels") => Self::Channels,
            Some("position") => Self::Position,
            Some("navigation") => Self::Navigation,
            Some("candidates") => Self::Candidates,
            _ => Self::Overview,
        }
    }

    fn for_plot(plot: PlotFocus) -> Self {
        match plot {
            PlotFocus::Sky => Self::Overview,
            PlotFocus::Cn0 => Self::Channels,
            PlotFocus::Position | PlotFocus::Velocity | PlotFocus::Altitude => Self::Position,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HistoryFilter {
    All,
    Completed,
    NeedsAttention,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlotFocus {
    Sky,
    Position,
    Cn0,
    Velocity,
    Altitude,
}

#[derive(Clone, Copy)]
enum PlotPanelLayout {
    Standard,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideDrawer {
    Settings,
    CandidateComparison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsCategory {
    Input,
    Acquisition,
    Tracking,
    Pvt,
    Performance,
}

impl SettingsCategory {
    const ALL: [Self; 5] = [
        Self::Input,
        Self::Acquisition,
        Self::Tracking,
        Self::Pvt,
        Self::Performance,
    ];

    fn label(self, language: InterfaceLanguage) -> &'static str {
        match (self, language) {
            (Self::Input, InterfaceLanguage::SimplifiedChinese) => "输入",
            (Self::Acquisition, InterfaceLanguage::SimplifiedChinese) => "捕获",
            (Self::Tracking, InterfaceLanguage::SimplifiedChinese) => "跟踪",
            (Self::Pvt, InterfaceLanguage::SimplifiedChinese) => "PVT",
            (Self::Performance, InterfaceLanguage::SimplifiedChinese) => "性能",
            (Self::Input, InterfaceLanguage::English) => "Input",
            (Self::Acquisition, InterfaceLanguage::English) => "Acquisition",
            (Self::Tracking, InterfaceLanguage::English) => "Tracking",
            (Self::Pvt, InterfaceLanguage::English) => "PVT",
            (Self::Performance, InterfaceLanguage::English) => "Performance",
        }
    }

    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("acquisition") => Self::Acquisition,
            Some("tracking") => Self::Tracking,
            Some("pvt") => Self::Pvt,
            Some("performance") => Self::Performance,
            _ => Self::Input,
        }
    }

    fn description(self, language: InterfaceLanguage) -> &'static str {
        match (self, language) {
            (Self::Input, InterfaceLanguage::SimplifiedChinese) => "采样、IQ 与导航来源",
            (Self::Acquisition, InterfaceLanguage::SimplifiedChinese) => "冷启动与周期性重捕",
            (Self::Tracking, InterfaceLanguage::SimplifiedChinese) => "环路、锁定与信号质量",
            (Self::Pvt, InterfaceLanguage::SimplifiedChinese) => "输出、滤波与完整性",
            (Self::Performance, InterfaceLanguage::SimplifiedChinese) => "并行处理策略",
            (Self::Input, InterfaceLanguage::English) => "Sampling, IQ, and navigation source",
            (Self::Acquisition, InterfaceLanguage::English) => {
                "Cold start and scheduled reacquisition"
            }
            (Self::Tracking, InterfaceLanguage::English) => "Loops, lock state, and signal quality",
            (Self::Pvt, InterfaceLanguage::English) => "Output, filtering, and integrity",
            (Self::Performance, InterfaceLanguage::English) => "Parallel processing policy",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Input => IconName::FolderOpen,
            Self::Acquisition => IconName::Search,
            Self::Tracking => IconName::Inspector,
            Self::Pvt => IconName::ChartPie,
            Self::Performance => IconName::LayoutDashboard,
        }
    }
}

impl PlotFocus {
    fn id(self) -> &'static str {
        match self {
            Self::Sky => "sky",
            Self::Position => "position",
            Self::Cn0 => "cn0",
            Self::Velocity => "velocity",
            Self::Altitude => "altitude",
        }
    }
}

impl HistoryFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Completed,
            Self::Completed => Self::NeedsAttention,
            Self::NeedsAttention => Self::All,
        }
    }

    fn label(self, language: InterfaceLanguage) -> &'static str {
        match (self, language) {
            (Self::All, InterfaceLanguage::SimplifiedChinese) => "全部",
            (Self::Completed, InterfaceLanguage::SimplifiedChinese) => "已完成",
            (Self::NeedsAttention, InterfaceLanguage::SimplifiedChinese) => "需处理",
            (Self::All, InterfaceLanguage::English) => "All runs",
            (Self::Completed, InterfaceLanguage::English) => "Completed",
            (Self::NeedsAttention, InterfaceLanguage::English) => "Needs attention",
        }
    }
}

impl ThemePreference {
    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl InterfaceLanguage {
    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("en") | Some("english") => Self::English,
            _ => Self::SimplifiedChinese,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::SimplifiedChinese => "zh-CN",
            Self::English => "en",
        }
    }
}

/// One executable target behind a ⌘K palette result (issue 17 item 1):
/// either a global command or the selection of a concrete run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaletteCommand {
    NewAnalysis,
    OpenRunLibrary,
    OpenWorkspace,
    OpenInterfaceSettings,
    ToggleTheme,
    ReturnToLive,
    SelectRun(Uuid),
}

/// Caps the run entries in the palette so a large library stays navigable.
const PALETTE_RUN_LIMIT: usize = 12;

pub struct GnsscopeApp {
    settings: GuiProcessingSettings,
    library: Option<RunLibrary>,
    persistence_writer: Option<RunPersistenceWriter>,
    history: Vec<ProcessingRun>,
    queue: VecDeque<ProcessingRun>,
    active_run: Option<ProcessingRun>,
    live_run_id: Option<Uuid>,
    selected_run_id: Option<Uuid>,
    worker: Option<ActiveWorker>,
    worker_receiver: Option<Receiver<WorkerEvent>>,
    poll_scheduled: bool,
    channels: Vec<GuiChannelStatus>,
    epochs: Vec<GuiPvtViewModel>,
    epoch_channels: Vec<Vec<GuiChannelStatus>>,
    historical_epochs: Vec<GuiPvtViewModel>,
    historical_channels: Vec<Vec<GuiChannelStatus>>,
    diagnostic_events: Vec<GuiDiagnosticEvent>,
    historical_diagnostic_events: Vec<GuiDiagnosticEvent>,
    selected_epoch: Option<usize>,
    timeline: Entity<SliderState>,
    /// Cursor offset from the start of the visible timeline window, as a
    /// record fraction. `None` centers the cursor in the window; the seek
    /// slider pins it to the pointer's track fraction so dragging moves the
    /// playhead under the pointer instead of re-centering the window away
    /// from it (which froze the playhead at the track middle when zoomed).
    timeline_cursor_offset: Option<f32>,
    history_search: Entity<InputState>,
    gallery_input: Entity<InputState>,
    /// Precise epoch jump field rendered over the timeline's epoch counter.
    epoch_jump_input: Entity<InputState>,
    /// Whether the precise jump popover is open for the selected run.
    epoch_jump_editing: bool,
    setting_inputs: BTreeMap<&'static str, Entity<InputState>>,
    setting_parse_errors: BTreeMap<&'static str, &'static str>,
    history_filter: HistoryFilter,
    /// Hovered PRN (consumed by `plots::sky` for the sky-plot detail line).
    pub(crate) hovered_prn: Option<u8>,
    /// Pinned (clicked) PRN; hover takes display precedence over pin.
    pub(crate) pinned_prn: Option<u8>,
    focused_plot: Option<PlotFocus>,
    follow_live: bool,
    theme_preference: ThemePreference,
    interface_language: InterfaceLanguage,
    page: AppPage,
    workspace_view: WorkspaceView,
    system_dark: bool,
    side_drawer: Option<SideDrawer>,
    settings_category: SettingsCategory,
    compact_library_expanded: bool,
    runs_sidebar_visible: bool,
    run_configuration_visible: bool,
    timeline_visible: bool,
    timeline_zoom: usize,
    component_gallery: bool,
    gallery_selected_tab: usize,
    gallery_switch_checked: bool,
    gallery_activation_count: usize,
    status_message: String,
    /// Deterministic preview fixtures story mode (`GNSSCOPE_PREVIEW=1`);
    /// gates env-only story behavior such as skipping the source-file
    /// existence check for fixture paths that are not real files.
    preview_mode: bool,
    /// Real input hosting the title-bar command field (issue 17 item 1).
    command_input: Entity<InputState>,
    /// Whether the ⌘K command palette overlay is open.
    command_palette_open: bool,
    /// Keyboard-highlighted palette entry index (filtered view).
    command_highlight: Option<usize>,
    /// Global keystroke interceptor driving ⌘K / palette navigation.
    command_interceptor: Option<gpui_kit::Subscription>,
}

impl GnsscopeApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Visual fixtures must never recover or mutate the user's real run library.
        let is_preview = std::env::var("GNSSCOPE_PREVIEW")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        let library =
            if is_preview {
                None
            } else if let Some(path) = std::env::var_os("GNSSCOPE_TEST_LIBRARY") {
                Some(RunLibrary::open_validation(PathBuf::from(path)).expect(
                    "GNSSCOPE_TEST_LIBRARY must name an absolute, marked validation database",
                ))
            } else {
                RunLibrary::open_default().ok()
            };
        Self::with_library(window, cx, library, is_preview)
    }

    fn with_library(
        window: &mut Window,
        cx: &mut Context<Self>,
        library: Option<RunLibrary>,
        is_preview: bool,
    ) -> Self {
        let history = library
            .as_ref()
            .and_then(|library| library.list_runs().ok())
            .unwrap_or_default();
        let queue_order = library
            .as_ref()
            .and_then(|library| library.preference("queue_order").ok().flatten())
            .and_then(|value| serde_json::from_str::<Vec<Uuid>>(&value).ok())
            .unwrap_or_default();
        let queue = ordered_queue(&history, &queue_order);
        let stored_theme = library
            .as_ref()
            .and_then(|library| library.preference("theme").ok().flatten());
        let theme_preference = std::env::var("GNSSCOPE_THEME")
            .ok()
            .as_deref()
            .map(|value| ThemePreference::parse(Some(value)))
            .unwrap_or_else(|| ThemePreference::parse(stored_theme.as_deref()));
        let stored_language = library
            .as_ref()
            .and_then(|library| library.preference("interface_language").ok().flatten());
        let interface_language = std::env::var("GNSSCOPE_LANGUAGE")
            .ok()
            .as_deref()
            .map(|value| InterfaceLanguage::parse(Some(value)))
            .unwrap_or_else(|| InterfaceLanguage::parse(stored_language.as_deref()));
        match theme_preference {
            ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
            ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
        let system_dark = cx.theme().mode.is_dark();
        apply_component_theme(
            Theme::global_mut(cx),
            if system_dark {
                Palette::dark()
            } else {
                Palette::light()
            },
        );
        let timeline_visible = library
            .as_ref()
            .and_then(|library| library.preference("timeline_visible").ok().flatten())
            .is_none_or(|value| value != "false");
        let runs_sidebar_visible = library
            .as_ref()
            .and_then(|library| library.preference("runs_sidebar_visible").ok().flatten())
            .is_none_or(|value| value != "false");
        let run_configuration_visible = library
            .as_ref()
            .and_then(|library| {
                library
                    .preference("run_configuration_visible")
                    .ok()
                    .flatten()
            })
            .is_none_or(|value| value != "false");
        let iq_path = std::env::var_os("GNSSCOPE_IQ_PATH").map(PathBuf::from);
        let settings = match (iq_path, std::env::var("GNSSCOPE_PRESET").ok().as_deref()) {
            (Some(path), Some("cttc")) => GuiProcessingSettings::cttc_spain(path),
            (Some(path), _) => GuiProcessingSettings {
                iq_file_path: Some(path),
                ..Default::default()
            },
            (None, _) => GuiProcessingSettings::default(),
        };
        let timeline = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(0.1)
                .default_value(100.0)
        });
        cx.subscribe(&timeline, |this, _, event, cx| {
            if let SliderEvent::Change(value) = event {
                this.select_epoch_percent(value.end());
                cx.notify();
            }
        })
        .detach();
        let history_search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(match interface_language {
                    // Distinct role from the title-bar command field: this
                    // input only filters the Runs-dock list (spec §3.1/§3.2).
                    InterfaceLanguage::SimplifiedChinese => "筛选解算任务…",
                    InterfaceLanguage::English => "Filter runs…",
                })
                .clean_on_escape()
        });
        cx.subscribe(&history_search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        // Real command field behind the ⌘K palette (issue 17 item 1). Typing
        // here filters the palette's command/run results.
        let command_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(match interface_language {
                    InterfaceLanguage::SimplifiedChinese => "搜索命令或解算任务…",
                    InterfaceLanguage::English => "Search commands or runs…",
                })
                .clean_on_escape()
        });
        cx.subscribe(&command_input, |this, _, event: &InputEvent, cx| {
            match event {
                InputEvent::Change => {
                    // Typing opens/refreshes the palette and resets the
                    // keyboard highlight to the first result.
                    this.command_palette_open = true;
                    this.command_highlight = Some(0);
                    cx.notify();
                }
                InputEvent::Focus => {
                    // Clicking/tabbing into the command field opens the
                    // palette so the field always has a visible target.
                    this.command_palette_open = true;
                    this.command_highlight = Some(0);
                    cx.notify();
                }
                // Enter/Escape are consumed by the global keystroke
                // interceptor while the palette is open (it owns window
                // access for command execution).
                InputEvent::PressEnter { .. } | InputEvent::Blur => {}
            }
        })
        .detach();
        let gallery_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match interface_language {
                InterfaceLanguage::SimplifiedChinese => "输入状态示例",
                InterfaceLanguage::English => "Input state example",
            })
        });
        let epoch_jump_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match interface_language {
                InterfaceLanguage::SimplifiedChinese => "输入历元编号",
                InterfaceLanguage::English => "Enter epoch number",
            })
        });
        // The precise jump field commits on Enter and closes on blur. The seek
        // slider value is deliberately not refreshed here: a subscription has
        // no `&mut Window`, and the slider is pointer-position driven, so it
        // self-corrects on the next interaction.
        cx.subscribe(
            &epoch_jump_input,
            |this, input, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let value = input.read(cx).value().to_string();
                    let count = this.display_epochs().len();
                    if let Some(index) = epoch_jump_index(&value, count) {
                        this.selected_epoch = Some(index);
                        this.follow_live = false;
                        this.timeline_cursor_offset = None;
                    }
                    this.epoch_jump_editing = false;
                    cx.notify();
                }
                InputEvent::Blur if this.epoch_jump_editing => {
                    this.epoch_jump_editing = false;
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        let mut setting_inputs = BTreeMap::new();
        for &field in EDITABLE_SETTING_FIELDS {
            let input = cx.new(|cx| {
                InputState::new(window, cx).default_value(setting_text_value(&settings, field))
            });
            cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value().to_string();
                    this.apply_setting_text(field, &value);
                    cx.notify();
                }
            })
            .detach();
            setting_inputs.insert(field, input);
        }
        let initial_run_id = std::env::var_os("GNSSCOPE_START_DRAFT")
            .is_none()
            .then(|| {
                history
                    .iter()
                    .find(|run| run.status != RunStatus::Queued)
                    .map(|run| run.id)
            })
            .flatten();
        let initial_stored_epochs = initial_run_id
            .and_then(|id| library.as_ref().and_then(|library| library.epochs(id).ok()))
            .unwrap_or_default();
        let historical_channels = initial_stored_epochs
            .iter()
            .map(StoredEpoch::channel_view_models)
            .collect::<Vec<_>>();
        let historical_epochs = initial_stored_epochs
            .iter()
            .map(StoredEpoch::to_view_model)
            .collect::<Vec<_>>();
        let historical_diagnostic_events = initial_run_id
            .and_then(|id| library.as_ref().and_then(|library| library.events(id).ok()))
            .unwrap_or_default()
            .iter()
            .filter_map(|event| event.to_view_model())
            .collect();
        let selected_epoch = historical_epochs.len().checked_sub(1);
        let persistence_writer = library
            .as_ref()
            .and_then(|library| library.spawn_persistence_writer().ok());
        let mut app = Self {
            settings,
            library,
            persistence_writer,
            history,
            queue,
            active_run: None,
            live_run_id: None,
            selected_run_id: initial_run_id,
            worker: None,
            worker_receiver: None,
            poll_scheduled: false,
            channels: Vec::new(),
            epochs: Vec::new(),
            epoch_channels: Vec::new(),
            historical_epochs,
            historical_channels,
            diagnostic_events: Vec::new(),
            historical_diagnostic_events,
            selected_epoch,
            timeline,
            timeline_cursor_offset: None,
            history_search,
            gallery_input,
            epoch_jump_input,
            epoch_jump_editing: false,
            setting_inputs,
            setting_parse_errors: BTreeMap::new(),
            history_filter: HistoryFilter::All,
            hovered_prn: None,
            pinned_prn: None,
            focused_plot: None,
            follow_live: true,
            theme_preference,
            interface_language,
            page: if std::env::var("GNSSCOPE_OPEN_INTERFACE_SETTINGS")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            {
                AppPage::InterfaceSettings
            } else if std::env::var("GNSSCOPE_OPEN_NEW_ANALYSIS")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            {
                AppPage::NewAnalysis
            } else if std::env::var("GNSSCOPE_OPEN_RUN_LIBRARY")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            {
                AppPage::RunLibrary
            } else {
                AppPage::Workspace
            },
            workspace_view: WorkspaceView::parse(
                std::env::var("GNSSCOPE_WORKSPACE_VIEW").ok().as_deref(),
            ),
            system_dark,
            side_drawer: if std::env::var("GNSSCOPE_OPEN_COMPARISON")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            {
                Some(SideDrawer::CandidateComparison)
            } else if std::env::var("GNSSCOPE_OPEN_SETTINGS")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            {
                Some(SideDrawer::Settings)
            } else {
                None
            },
            settings_category: SettingsCategory::parse(
                std::env::var("GNSSCOPE_SETTINGS_CATEGORY").ok().as_deref(),
            ),
            compact_library_expanded: false,
            runs_sidebar_visible,
            run_configuration_visible,
            timeline_visible,
            timeline_zoom: 1,
            component_gallery: std::env::var("GNSSCOPE_COMPONENT_GALLERY")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true")),
            gallery_selected_tab: 0,
            gallery_switch_checked: true,
            gallery_activation_count: 0,
            status_message: "请选择 IQ 文件开始解算".to_string(),
            preview_mode: is_preview,
            command_input,
            command_palette_open: false,
            command_highlight: None,
            command_interceptor: None,
        };
        if let Some(settings) = app.selected_run().map(|run| run.settings.clone()) {
            app.settings = settings;
            app.sync_setting_inputs(window, cx);
        }
        if is_preview {
            let (preview_epochs, preview_channels) = preview_epoch_history();
            let preview_id = Uuid::from_u128(0x2025_0521_1432_0700_0000_0000_0000_0001);
            let preview_source = PathBuf::from("2025-05-21_14-32-07_Run01.iqrun");
            let mut preview_settings = app.settings.clone();
            preview_settings.iq_file_path = Some(preview_source.clone());
            let preview_duration_seconds = 1_220.0;
            let preview_total_samples =
                (app.settings.sample_rate_hz * preview_duration_seconds) as u64;
            let preview_run = |id_offset: u128,
                               timestamp: &str,
                               source: &str,
                               status: RunStatus,
                               progress: f64,
                               pvt_epoch_count: usize| {
                let duration_seconds = match id_offset {
                    9 => 621.0,
                    8 => 936.0,
                    7 => 558.0,
                    6 => 663.0,
                    4 => 527.0,
                    _ => preview_duration_seconds,
                };
                let total_samples = (preview_settings.sample_rate_hz * duration_seconds) as u64;
                ProcessingRun {
                    id: Uuid::from_u128(preview_id.as_u128() + id_offset),
                    created_at: chrono::DateTime::parse_from_rfc3339(timestamp)
                        .expect("valid deterministic preview timestamp")
                        .with_timezone(&chrono::Utc),
                    source_path: PathBuf::from(source),
                    status,
                    progress,
                    processed_samples: (total_samples as f64 * progress) as u64,
                    total_samples,
                    pvt_epoch_count,
                    message: "Deterministic visual preview".to_string(),
                    settings: preview_settings.clone(),
                }
            };
            app.queue = vec![
                preview_run(
                    2,
                    "2025-05-21T14:45:00Z",
                    "2025-05-21_14-45-00_Run02.iqrun",
                    RunStatus::Queued,
                    0.0,
                    0,
                ),
                preview_run(
                    3,
                    "2025-05-21T15:00:00Z",
                    "2025-05-21_15-00-00_Run03.iqrun",
                    RunStatus::Queued,
                    0.0,
                    0,
                ),
            ]
            .into();
            app.history = vec![
                preview_run(
                    9,
                    "2025-05-21T13:50:00Z",
                    "2025-05-21_13-50-00_Run09.iqrun",
                    RunStatus::Completed,
                    1.0,
                    182,
                ),
                preview_run(
                    8,
                    "2025-05-21T13:20:00Z",
                    "2025-05-21_13-20-00_Run08.iqrun",
                    RunStatus::Completed,
                    1.0,
                    180,
                ),
                preview_run(
                    7,
                    "2025-05-21T12:40:00Z",
                    "2025-05-21_12-40-00_Run07.iqrun",
                    RunStatus::Completed,
                    1.0,
                    176,
                ),
                preview_run(
                    6,
                    "2025-05-21T12:05:00Z",
                    "2025-05-21_12-05-00_Run06.iqrun",
                    RunStatus::Completed,
                    1.0,
                    181,
                ),
                preview_run(
                    5,
                    "2025-05-21T11:30:00Z",
                    "2025-05-21_11-30-00_Run05.iqrun",
                    RunStatus::Failed,
                    // Honest failure story: the run died 252 s (00:04:12)
                    // into the full 1,220 s recording, so the strip draws
                    // the actual partial progress, never a full bar (§2.7).
                    252.0 / preview_duration_seconds,
                    74,
                ),
                preview_run(
                    4,
                    "2025-05-21T11:00:00Z",
                    "2025-05-21_11-00-00_Run04.iqrun",
                    RunStatus::Completed,
                    1.0,
                    179,
                ),
            ];
            app.active_run = Some(ProcessingRun {
                id: preview_id,
                created_at: chrono::DateTime::parse_from_rfc3339("2025-05-21T14:32:07Z")
                    .expect("valid deterministic preview timestamp")
                    .with_timezone(&chrono::Utc),
                source_path: preview_source,
                status: RunStatus::Processing,
                progress: 0.63,
                processed_samples: (preview_total_samples as f64 * 0.63) as u64,
                total_samples: preview_total_samples,
                pvt_epoch_count: preview_epochs.len(),
                message: "Streaming acquisition, tracking and PVT".to_string(),
                settings: preview_settings,
            });
            app.selected_run_id = Some(preview_id);
            app.live_run_id = Some(preview_id);
            app.epochs = preview_epochs;
            app.epoch_channels = preview_channels;
            app.diagnostic_events = preview_diagnostic_events();
            app.historical_epochs.clear();
            app.historical_channels.clear();
            app.historical_diagnostic_events.clear();
            app.selected_epoch = app.epochs.len().checked_sub(1);
            app.runs_sidebar_visible = true;
            app.run_configuration_visible = true;
            app.timeline_visible = true;
            app.status_message = "确定性预览".to_string();
        }
        if std::env::var_os("GNSSCOPE_AUTORUN").is_some() {
            app.enqueue(cx);
        }
        app.install_command_interceptor(cx);
        app.schedule_poll(cx);
        app
    }

    fn palette(&self) -> Palette {
        let dark = match self.theme_preference {
            ThemePreference::System => self.system_dark,
            ThemePreference::Light => false,
            ThemePreference::Dark => true,
        };
        if dark {
            Palette::dark()
        } else {
            Palette::light()
        }
    }

    fn set_theme_preference(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_preference = preference;
        match self.theme_preference {
            ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
            ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
        self.system_dark = cx.theme().mode.is_dark();
        apply_component_theme(Theme::global_mut(cx), self.palette());
        window.refresh();
        if let Some(library) = &self.library {
            let _ = library.set_preference("theme", self.theme_preference.value());
        }
    }

    fn set_interface_language(
        &mut self,
        language: InterfaceLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.interface_language = language;
        self.history_search.update(cx, |input, cx| {
            input.set_placeholder(
                match language {
                    InterfaceLanguage::SimplifiedChinese => "筛选解算任务…",
                    InterfaceLanguage::English => "Filter runs…",
                },
                window,
                cx,
            );
        });
        self.command_input.update(cx, |input, cx| {
            input.set_placeholder(
                match language {
                    InterfaceLanguage::SimplifiedChinese => "搜索命令或解算任务…",
                    InterfaceLanguage::English => "Search commands or runs…",
                },
                window,
                cx,
            );
        });
        if let Some(library) = &self.library {
            let _ = library.set_preference("interface_language", language.value());
        }
    }

    fn localized(&self, simplified_chinese: &'static str, english: &'static str) -> &'static str {
        match self.interface_language {
            InterfaceLanguage::SimplifiedChinese => simplified_chinese,
            InterfaceLanguage::English => english,
        }
    }

    fn history_filter_menu_item(
        app: Entity<Self>,
        filter: HistoryFilter,
        current: HistoryFilter,
        language: InterfaceLanguage,
    ) -> PopupMenuItem {
        PopupMenuItem::new(filter.label(language))
            .checked(filter == current)
            .on_click(move |_, _, cx| {
                app.update(cx, |this, cx| {
                    this.history_filter = filter;
                    cx.notify();
                });
            })
    }

    fn iq_format_menu_item(
        app: Entity<Self>,
        format: crate::GuiIqFormat,
        current: crate::GuiIqFormat,
    ) -> PopupMenuItem {
        PopupMenuItem::new(format!("{format:?}"))
            .checked(format == current)
            .on_click(move |_, _, cx| {
                app.update(cx, |this, cx| {
                    this.settings.iq_format = format;
                    cx.notify();
                });
            })
    }

    fn set_timeline_visibility(&mut self, visible: bool) {
        self.timeline_visible = visible;
        if let Some(library) = &self.library {
            let _ =
                library.set_preference("timeline_visible", if visible { "true" } else { "false" });
        }
    }

    fn set_runs_sidebar_visibility(&mut self, visible: bool) {
        self.runs_sidebar_visible = visible;
        if let Some(library) = &self.library {
            let _ = library.set_preference(
                "runs_sidebar_visible",
                if visible { "true" } else { "false" },
            );
        }
    }

    fn set_run_configuration_visibility(&mut self, visible: bool) {
        self.run_configuration_visible = visible;
        if let Some(library) = &self.library {
            let _ = library.set_preference(
                "run_configuration_visible",
                if visible { "true" } else { "false" },
            );
        }
    }

    fn schedule_poll(&mut self, cx: &mut Context<Self>) {
        if self.poll_scheduled || self.worker_receiver.is_none() {
            return;
        }
        self.poll_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.poll_scheduled = false;
                    let drained_events = this.drain_worker_events();
                    let (notify, continue_polling) =
                        poll_decision(this.worker_receiver.is_some(), drained_events);
                    if continue_polling {
                        this.schedule_poll(cx);
                    }
                    if notify {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn select_file(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择 GNSS IQ 文件".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let (Some(this), Some(path)) = (this.upgrade(), paths.into_iter().next()) {
                    this.update(cx, |this, cx| {
                        let detected_labsat = this.settings.select_iq_file(path);
                        this.selected_run_id = None;
                        this.historical_epochs.clear();
                        this.historical_channels.clear();
                        this.historical_diagnostic_events.clear();
                        this.selected_epoch = None;
                        this.status_message = if detected_labsat {
                            this.localized(
                                "已读取 LabSat 元数据：10.23 MHz · Int8 I/Q · 零中频",
                                "LabSat metadata loaded: 10.23 MHz · Int8 I/Q · zero IF",
                            )
                        } else {
                            this.localized("IQ 文件已就绪", "IQ file ready")
                        }
                        .to_string();
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn start_new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.compact_library_expanded = false;
        self.settings = GuiProcessingSettings::default();
        self.selected_run_id = None;
        self.historical_epochs.clear();
        self.historical_channels.clear();
        self.historical_diagnostic_events.clear();
        self.selected_epoch = None;
        self.follow_live = false;
        self.page = AppPage::NewAnalysis;
        self.side_drawer = None;
        self.focused_plot = None;
        self.sync_setting_inputs(window, cx);
        self.status_message = self
            .localized(
                "请选择 IQ 文件创建任务",
                "Select an IQ file to create a task",
            )
            .to_string();
        self.select_file(cx);
    }

    fn reset_receiver_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let iq_file_path = self.settings.iq_file_path.clone();
        self.settings = GuiProcessingSettings {
            iq_file_path,
            ..Default::default()
        };
        self.sync_setting_inputs(window, cx);
        self.status_message = self
            .localized(
                "接收机配置已恢复默认",
                "Receiver settings restored to defaults",
            )
            .to_string();
    }

    fn apply_setting_text(&mut self, field: &'static str, value: &str) {
        let value = value.trim();
        macro_rules! assign {
            ($type:ty, $name:ident) => {{
                if let Ok(parsed) = value.parse::<$type>() {
                    self.settings.$name = parsed;
                    true
                } else {
                    false
                }
            }};
        }
        let parsed = match field {
            "sample_rate_hz" => match value.parse::<f64>() {
                Ok(parsed) => {
                    self.settings.sample_rate_hz = parsed * 1e6;
                    true
                }
                Err(_) => false,
            },
            "intermediate_frequency_hz" => match value.parse::<f64>() {
                Ok(parsed) => {
                    self.settings.intermediate_frequency_hz = parsed * 1e6;
                    true
                }
                Err(_) => false,
            },
            "chunk_ms" => assign!(usize, chunk_ms),
            "pvt_output_rate_ms" => assign!(u64, pvt_output_rate_ms),
            "acquisition_window_ms" => assign!(usize, acquisition_window_ms),
            "acquisition_noncoherent_integrations" => {
                assign!(usize, acquisition_noncoherent_integrations)
            }
            "acquisition_threshold" => assign!(f64, acquisition_threshold),
            "cold_acquisition_interval_ms" => assign!(u64, cold_acquisition_interval_ms),
            "acquisition_doppler_max_hz" => assign!(f64, acquisition_doppler_max_hz),
            "acquisition_doppler_step_hz" => assign!(f64, acquisition_doppler_step_hz),
            "reacquisition_interval_ms" => assign!(u64, reacquisition_interval_ms),
            "max_acquisition_prns_per_scan" => assign!(usize, max_acquisition_prns_per_scan),
            "dll_bandwidth_hz" => assign!(f64, dll_bandwidth_hz),
            "pll_bandwidth_hz" => assign!(f64, pll_bandwidth_hz),
            "early_late_spacing_chips" => assign!(f64, early_late_spacing_chips),
            "cn0_samples" => assign!(usize, cn0_samples),
            "cn0_smoother_alpha" => assign!(f64, cn0_smoother_alpha),
            "minimum_pll_lock_metric" => assign!(f64, minimum_pll_lock_metric),
            "minimum_cn0_db_hz" => assign!(f64, minimum_cn0_db_hz),
            "probation_good_epochs" => assign!(usize, probation_good_epochs),
            "rinex_nav_path" => {
                self.settings.rinex_nav_path = (!value.is_empty()).then(|| PathBuf::from(value));
                true
            }
            "fixed_prns" => parse_fixed_prns(value)
                .map(|parsed| self.settings.fixed_prns = parsed)
                .is_ok(),
            "initial_position_lla_deg_m" => parse_initial_lla(value)
                .map(|parsed| self.settings.initial_position_lla_deg_m = parsed)
                .is_ok(),
            _ => return,
        };
        if parsed {
            self.setting_parse_errors.remove(field);
        } else {
            self.setting_parse_errors.insert(
                field,
                self.localized(
                    "请输入有效数值或规定格式",
                    "Enter a valid value in the required format",
                ),
            );
        }
    }

    fn sync_setting_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.setting_parse_errors.clear();
        let updates = self
            .setting_inputs
            .iter()
            .map(|(&field, input)| {
                (
                    input.clone(),
                    SharedString::from(setting_text_value(&self.settings, field)),
                )
            })
            .collect::<Vec<_>>();
        for (input, value) in updates {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }

    fn toggle_setting(&mut self, field: &'static str) {
        match field {
            "average_doppler_over_pvt_epoch" => {
                self.settings.average_doppler_over_pvt_epoch =
                    !self.settings.average_doppler_over_pvt_epoch
            }
            "carrier_aiding" => self.settings.carrier_aiding = !self.settings.carrier_aiding,
            "parallel_acquisition" => {
                self.settings.parallel_acquisition = !self.settings.parallel_acquisition
            }
            "parallel_tracking" => {
                self.settings.parallel_tracking = !self.settings.parallel_tracking
            }
            "enable_raim" => self.settings.enable_raim = !self.settings.enable_raim,
            "use_rinex_nav_environment" => {
                self.settings.use_rinex_nav_environment = !self.settings.use_rinex_nav_environment
            }
            "auto_discover_colocated_nav" => {
                self.settings.auto_discover_colocated_nav =
                    !self.settings.auto_discover_colocated_nav
            }
            "enable_carrier_smoothing" => {
                self.settings.enable_carrier_smoothing = !self.settings.enable_carrier_smoothing
            }
            "enable_kalman" => self.settings.enable_kalman = !self.settings.enable_kalman,
            "enable_integer_ms_ambiguity_resolution" => {
                self.settings.enable_integer_ms_ambiguity_resolution =
                    !self.settings.enable_integer_ms_ambiguity_resolution
            }
            _ => {}
        }
    }

    fn enqueue(&mut self, cx: &mut Context<Self>) {
        if let Some((field, message)) = self.setting_parse_errors.iter().next() {
            self.status_message = format!("{field}: {message}");
            return;
        }
        self.settings = self.settings.normalized_for_processing();
        if let Some(issue) = crate::state::validate_settings(&self.settings).first() {
            self.status_message = format!("{}: {}", issue.field, issue.message);
            return;
        }
        let Some(library) = &self.library else {
            self.status_message = "运行记录数据库不可用".to_string();
            return;
        };
        match library.create_run(&self.settings) {
            Ok(run) => {
                self.history.insert(0, run.clone());
                self.selected_run_id = Some(run.id);
                if self.active_run.is_some() {
                    // The new selected run has no results yet. Do not attribute the
                    // previously inspected history to it; active-run buffers stay intact.
                    self.historical_epochs.clear();
                    self.historical_channels.clear();
                    self.historical_diagnostic_events.clear();
                    self.selected_epoch = None;
                    self.follow_live = false;
                    self.queue.push_back(run);
                    self.persist_queue_order();
                    self.status_message = "任务已加入等待队列".to_string();
                } else {
                    self.start_run(run);
                    self.schedule_poll(cx);
                }
            }
            Err(error) => self.status_message = format!("无法创建运行记录：{error}"),
        }
    }

    fn enqueue_selected_or_draft(&mut self, cx: &mut Context<Self>) {
        self.enqueue(cx);
    }

    fn start_run(&mut self, mut run: ProcessingRun) {
        let (sender, receiver) = mpsc::sync_channel(WORKER_EVENT_CAPACITY);
        let worker = spawn_streaming_worker(run.settings.clone(), sender);
        run.status = RunStatus::Processing;
        run.message = "初始化接收机".to_string();
        persist_progress(
            self.persistence_writer.as_ref(),
            self.library.as_ref(),
            PersistenceProgress {
                run_id: run.id,
                status: RunStatus::Processing,
                processed_samples: 0,
                total_samples: 0,
                pvt_epoch_count: 0,
                message: &run.message,
            },
        );
        let run_id = run.id;
        self.active_run = Some(run);
        self.live_run_id = Some(run_id);
        if self.selected_run_id.is_none() {
            self.selected_run_id = Some(run_id);
        }
        self.worker = Some(worker);
        self.worker_receiver = Some(receiver);
        self.channels.clear();
        self.epochs.clear();
        self.epoch_channels.clear();
        self.diagnostic_events.clear();
        self.selected_epoch = None;
        self.follow_live = true;
        self.status_message = "初始化接收机".to_string();
    }

    fn cancel_active(&mut self) {
        if let Some(worker) = &self.worker {
            worker.cancel();
            self.status_message = "将在当前数据块结束后取消".to_string();
        }
    }

    fn cancel_queued(&mut self, id: Uuid) {
        let Some(index) = self.queue.iter().position(|run| run.id == id) else {
            return;
        };
        let run = self.queue.remove(index).expect("queued run index");
        self.persist_queue_order();
        if let Some(library) = &self.library {
            let _ = library.update_progress(id, RunStatus::Cancelled, 0, 0, 0, "排队任务已取消");
            self.history = library.list_runs().unwrap_or_default();
        }
        if self.selected_run_id == Some(id) {
            self.selected_run_id = None;
        }
        self.status_message = format!(
            "已取消排队任务 {}",
            run.source_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
    }

    fn move_queued(&mut self, id: Uuid, direction: i32) {
        let Some(index) = self.queue.iter().position(|run| run.id == id) else {
            return;
        };
        if move_queue_item(&mut self.queue, index, direction) {
            self.persist_queue_order();
            self.status_message = "已更新任务队列顺序".to_string();
        }
    }

    fn persist_queue_order(&self) {
        let Some(library) = &self.library else {
            return;
        };
        let order = self.queue.iter().map(|run| run.id).collect::<Vec<_>>();
        if let Ok(value) = serde_json::to_string(&order) {
            let _ = library.set_preference("queue_order", &value);
        }
    }

    fn reveal_source_file(&mut self, id: Uuid) {
        let Some(run) = self.history.iter().find(|run| run.id == id) else {
            return;
        };
        let path = run.source_path.clone();
        if !path.exists() {
            self.status_message = "源 IQ 文件已移动或删除".to_string();
            return;
        }
        #[cfg(target_os = "windows")]
        let result = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn();
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&path)
            .spawn();
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let result = std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(std::path::Path::new(".")))
            .spawn();
        self.status_message = if result.is_ok() {
            "已在文件管理器中定位源文件".to_string()
        } else {
            "无法打开文件管理器".to_string()
        };
    }

    fn request_delete_historical_run(
        &mut self,
        id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.history.iter().find(|run| run.id == id) else {
            return;
        };
        if matches!(run.status, RunStatus::Processing | RunStatus::Queued) {
            return;
        }
        let detail = format!("{}\nID: {}\n\n{}", run.source_path.display(), id,
            self.localized("此操作无法撤销。仅删除此运行记录及结果，不删除源 IQ 文件。", "This cannot be undone. Only this run and its results will be deleted; the source IQ file is kept."));
        let answer = window.prompt(
            gpui_kit::PromptLevel::Warning,
            self.localized("删除运行记录？", "Delete this run?"),
            Some(&detail),
            &[
                self.localized("取消", "Cancel"),
                self.localized("删除运行", "Delete run"),
            ],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update(cx, |this, cx| {
                    // Capture identity at request time; never use the current selection.
                    this.delete_historical_run(id);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn select_context_run(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        // Right-click identifies the target without closing a narrow Runs overlay.
        let expanded = self.compact_library_expanded;
        self.page = AppPage::Workspace;
        self.select_run(id, window, cx);
        self.compact_library_expanded = expanded;
        cx.notify();
    }

    fn history_context_menu(
        &self,
        id: Uuid,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let app = cx.entity();
        let palette = self.palette();
        let reveal = self.localized("在 Finder 中显示", "Reveal source file");
        let delete = self.localized("删除运行记录…", "Delete run…");
        move |menu, window, cx| {
            app.update(cx, |app, cx| app.select_context_run(id, window, cx));
            let reveal_app = app.clone();
            let delete_app = app.clone();
            menu.min_w(px(208.))
                .max_w(px(240.))
                .item(
                    crate::ui::run_rows::history_menu_item(
                        reveal,
                        IconName::FolderOpen,
                        false,
                        palette,
                    )
                    .on_click(move |_, _, cx| {
                        reveal_app.update(cx, |app, cx| {
                            app.reveal_source_file(id);
                            cx.notify();
                        });
                    }),
                )
                .separator()
                .item(
                    crate::ui::run_rows::history_menu_item(delete, IconName::Delete, true, palette)
                        .on_click(move |_, window, cx| {
                            delete_app.update(cx, |app, cx| {
                                app.request_delete_historical_run(id, window, cx)
                            });
                        }),
                )
        }
    }

    fn delete_historical_run(&mut self, id: Uuid) {
        if self.active_run.as_ref().is_some_and(|run| run.id == id) {
            return;
        }
        if self.queue.iter().any(|run| run.id == id) {
            return;
        }
        let Some(library) = &self.library else {
            return;
        };
        if library.delete_run(id).is_err() {
            self.status_message = "无法删除历史运行".to_string();
            return;
        }
        self.history = library.list_runs().unwrap_or_default();
        self.queue.retain(|run| run.id != id);
        self.persist_queue_order();
        if self.selected_run_id == Some(id) {
            self.selected_run_id = None;
            self.historical_epochs.clear();
            self.historical_channels.clear();
            self.historical_diagnostic_events.clear();
            self.selected_epoch = None;
        }
        self.status_message = "历史运行已删除，源 IQ 文件未修改".to_string();
    }

    fn drain_worker_events(&mut self) -> usize {
        let events = self
            .worker_receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let event_count = events.len();
        for event in events {
            self.apply_worker_event(event);
        }
        event_count
    }

    fn apply_worker_event(&mut self, event: WorkerEvent) {
        let Some(run_id) = self.active_run.as_ref().map(|run| run.id) else {
            return;
        };
        match event {
            WorkerEvent::Started { total_samples } => {
                if let Some(run) = self.active_run.as_mut() {
                    run.total_samples = total_samples;
                    run.message = "捕获与跟踪".to_string();
                }
            }
            WorkerEvent::Progress {
                processed_samples,
                total_samples,
                phase,
            } => {
                if let Some(run) = self.active_run.as_mut() {
                    run.processed_samples = processed_samples;
                    run.total_samples = total_samples;
                    run.progress = if total_samples == 0 {
                        0.0
                    } else {
                        processed_samples as f64 / total_samples as f64
                    };
                    run.message = phase.clone();
                    run.pvt_epoch_count = self.epochs.len();
                    persist_progress(
                        self.persistence_writer.as_ref(),
                        self.library.as_ref(),
                        PersistenceProgress {
                            run_id: run.id,
                            status: RunStatus::Processing,
                            processed_samples,
                            total_samples,
                            pvt_epoch_count: self.epochs.len(),
                            message: &phase,
                        },
                    );
                }
                self.status_message = phase;
            }
            WorkerEvent::Epoch { pvt, channels } => {
                let epoch_ordinal = self.epochs.len();
                let pvt = *pvt;
                persist_epoch(
                    self.persistence_writer.as_ref(),
                    self.library.as_ref(),
                    run_id,
                    epoch_ordinal,
                    &pvt,
                    &channels,
                );
                self.channels = channels.clone();
                self.epoch_channels.push(channels);
                self.epochs.push(pvt);
                if self.follow_live {
                    self.selected_run_id = self.live_run_id;
                    self.selected_epoch = Some(self.epochs.len() - 1);
                }
            }
            WorkerEvent::Channels(channels) => {
                self.channels = channels;
            }
            WorkerEvent::Diagnostic(event) => {
                persist_event(
                    self.persistence_writer.as_ref(),
                    self.library.as_ref(),
                    run_id,
                    self.diagnostic_events.len(),
                    &event,
                );
                self.diagnostic_events.push(event);
            }
            WorkerEvent::Cancelled {
                processed_samples,
                total_samples,
            } => {
                self.finish_active(
                    RunStatus::Cancelled,
                    processed_samples,
                    total_samples,
                    "已取消，部分结果已保留",
                );
            }
            WorkerEvent::Completed {
                processed_samples,
                total_samples,
            } => {
                self.finish_active(
                    RunStatus::Completed,
                    processed_samples,
                    total_samples,
                    "处理完成",
                );
            }
            WorkerEvent::Failed(message) => {
                let processed = self
                    .active_run
                    .as_ref()
                    .map(|run| run.processed_samples)
                    .unwrap_or(0);
                let total = self
                    .active_run
                    .as_ref()
                    .map(|run| run.total_samples)
                    .unwrap_or(0);
                self.finish_active(RunStatus::Failed, processed, total, &message);
            }
        }
    }

    fn finish_active(&mut self, status: RunStatus, processed: u64, total: u64, message: &str) {
        if let Some(run) = self.active_run.as_mut() {
            run.status = status;
            run.processed_samples = processed;
            run.total_samples = total;
            run.progress = if total == 0 {
                0.0
            } else {
                processed as f64 / total as f64
            };
            run.pvt_epoch_count = self.epochs.len();
            run.message = message.to_string();
            finalize_persistence(
                self.persistence_writer.as_ref(),
                self.library.as_ref(),
                PersistenceProgress {
                    run_id: run.id,
                    status,
                    processed_samples: processed,
                    total_samples: total,
                    pvt_epoch_count: self.epochs.len(),
                    message,
                },
            );
        }
        self.status_message = message.to_string();
        self.worker = None;
        self.worker_receiver = None;
        self.active_run = None;
        if let Some(next) = self.queue.pop_front() {
            self.persist_queue_order();
            self.start_run(next);
        }
        if let Some(library) = &self.library {
            self.history = library.list_runs().unwrap_or_default();
        }
    }

    fn current_pvt(&self) -> Option<&GuiPvtViewModel> {
        self.selected_epoch
            .and_then(|index| self.display_epochs().get(index))
            .or_else(|| self.display_epochs().last())
    }

    fn display_epochs(&self) -> &[GuiPvtViewModel] {
        if self.selected_run_id.is_some() && self.selected_run_id == self.live_run_id {
            &self.epochs
        } else {
            &self.historical_epochs
        }
    }

    fn display_channels(&self) -> &[GuiChannelStatus] {
        if self.selected_run_id.is_some() && self.selected_run_id == self.live_run_id {
            self.selected_epoch
                .and_then(|index| self.epoch_channels.get(index))
                .map(Vec::as_slice)
                .unwrap_or(&self.channels)
        } else {
            self.selected_epoch
                .and_then(|index| self.historical_channels.get(index))
                .map(Vec::as_slice)
                .unwrap_or(&[])
        }
    }

    fn display_epoch_channels(&self) -> &[Vec<GuiChannelStatus>] {
        if self.selected_run_id.is_some() && self.selected_run_id == self.live_run_id {
            &self.epoch_channels
        } else {
            &self.historical_channels
        }
    }

    fn display_diagnostic_events(&self) -> &[GuiDiagnosticEvent] {
        if self.selected_run_id.is_some() && self.selected_run_id == self.live_run_id {
            &self.diagnostic_events
        } else {
            &self.historical_diagnostic_events
        }
    }

    fn select_run(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.compact_library_expanded = false;
        if let Some(settings) = self
            .active_run
            .as_ref()
            .filter(|run| run.id == id)
            .or_else(|| self.history.iter().find(|run| run.id == id))
            .map(|run| run.settings.clone())
        {
            self.settings = settings;
            self.sync_setting_inputs(window, cx);
        }
        self.selected_run_id = Some(id);
        self.follow_live = self.live_run_id == Some(id) && self.active_run.is_some();
        self.timeline.update(cx, |timeline, cx| {
            timeline.set_value(100.0, window, cx);
        });
        if self.live_run_id == Some(id) {
            self.selected_epoch = self.epochs.len().checked_sub(1);
            return;
        }
        let stored = self
            .library
            .as_ref()
            .and_then(|library| library.epochs(id).ok())
            .unwrap_or_default();
        self.historical_channels = stored
            .iter()
            .map(StoredEpoch::channel_view_models)
            .collect();
        self.historical_epochs = stored.iter().map(StoredEpoch::to_view_model).collect();
        self.historical_diagnostic_events = self
            .library
            .as_ref()
            .and_then(|library| library.events(id).ok())
            .unwrap_or_default()
            .iter()
            .filter_map(|event| event.to_view_model())
            .collect();
        self.selected_epoch = self.historical_epochs.len().checked_sub(1);
    }

    fn return_to_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.live_run_id else {
            return;
        };
        self.selected_run_id = Some(id);
        self.selected_epoch = self.epochs.len().checked_sub(1);
        self.follow_live = self.active_run.is_some();
        self.timeline_cursor_offset = None;
        self.timeline.update(cx, |timeline, cx| {
            timeline.set_value(100.0, window, cx);
        });
    }

    // -----------------------------------------------------------------------
    // ⌘K command palette state machine (issue 17 item 1)
    // -----------------------------------------------------------------------

    /// Palette result rows: global commands first, then run entries (active
    /// run + history, capped). A run's hint is its localized status word; a
    /// command's hint is a keyboard affordance or `None` (never fabricated).
    fn palette_results(&self) -> Vec<(CommandResult, PaletteCommand)> {
        let mut rows: Vec<(CommandResult, PaletteCommand)> = Vec::new();
        let command = |rows: &mut Vec<(CommandResult, PaletteCommand)>,
                       title: &'static str,
                       hint: Option<&'static str>,
                       command: PaletteCommand| {
            rows.push((
                CommandResult {
                    title: title.to_string(),
                    hint,
                },
                command,
            ));
        };
        command(
            &mut rows,
            self.localized("新建分析", "New analysis"),
            None,
            PaletteCommand::NewAnalysis,
        );
        command(
            &mut rows,
            self.localized("运行库", "Run library"),
            None,
            PaletteCommand::OpenRunLibrary,
        );
        command(
            &mut rows,
            self.localized("工作台", "Workspace"),
            None,
            PaletteCommand::OpenWorkspace,
        );
        command(
            &mut rows,
            self.localized("界面设置", "Interface settings"),
            None,
            PaletteCommand::OpenInterfaceSettings,
        );
        command(
            &mut rows,
            self.localized("切换主题", "Toggle theme"),
            None,
            PaletteCommand::ToggleTheme,
        );
        if self.live_run_id.is_some() && self.selected_run_id != self.live_run_id {
            command(
                &mut rows,
                self.localized("返回实时解算", "Return to live run"),
                None,
                PaletteCommand::ReturnToLive,
            );
        }
        for run in self
            .active_run
            .iter()
            .chain(self.history.iter())
            .take(PALETTE_RUN_LIMIT)
        {
            let title = run
                .source_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| run.source_path.display().to_string());
            rows.push((
                CommandResult {
                    title,
                    hint: Some(self.run_status_word(run.status)),
                },
                PaletteCommand::SelectRun(run.id),
            ));
        }
        rows
    }

    /// Localized one-word run status, reused as the palette hint (never a fix
    /// type or a `Debug` string).
    fn run_status_word(&self, status: RunStatus) -> &'static str {
        match status {
            RunStatus::Completed => self.localized("已完成", "Completed"),
            RunStatus::Failed => self.localized("失败", "Failed"),
            RunStatus::Interrupted => self.localized("已中断", "Interrupted"),
            RunStatus::Cancelled => self.localized("已取消", "Cancelled"),
            RunStatus::Queued => self.localized("排队", "Queued"),
            _ => self.localized("处理中", "Processing"),
        }
    }

    /// The palette presentation snapshot for the current command input value.
    fn palette_view(&self, window: &Window, cx: &App) -> CommandPaletteView {
        let query = self.command_input.read(cx).value().to_string();
        let search_focused = self.command_input.focus_handle(cx).is_focused(window);
        CommandPaletteView {
            query,
            search_focused,
            highlighted: self.command_highlight,
            results: self
                .palette_results()
                .into_iter()
                .map(|(result, _)| result)
                .collect(),
            zh: matches!(
                self.interface_language,
                InterfaceLanguage::SimplifiedChinese
            ),
        }
    }

    /// The commands backing each currently-visible (filtered) row, in render
    /// order. Uses the same `matches_query` rule as the overlay so keyboard
    /// and pointer selection resolve to the same target.
    fn palette_filtered_commands(&self, cx: &App) -> Vec<PaletteCommand> {
        let query = self.command_input.read(cx).value().to_string();
        self.palette_results()
            .into_iter()
            .filter(|(result, _)| matches_query(&query, &result.title))
            .map(|(_, command)| command)
            .collect()
    }

    fn execute_palette_command(
        &mut self,
        command: PaletteCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            PaletteCommand::NewAnalysis => self.start_new_task(window, cx),
            PaletteCommand::OpenRunLibrary => {
                self.page = AppPage::RunLibrary;
                self.compact_library_expanded = false;
            }
            PaletteCommand::OpenWorkspace => {
                self.page = AppPage::Workspace;
                self.side_drawer = None;
            }
            PaletteCommand::OpenInterfaceSettings => {
                self.page = AppPage::InterfaceSettings;
                self.side_drawer = None;
                self.focused_plot = None;
            }
            PaletteCommand::ToggleTheme => {
                let dark = match self.theme_preference {
                    ThemePreference::System => self.system_dark,
                    ThemePreference::Light => false,
                    ThemePreference::Dark => true,
                };
                let next = if dark {
                    ThemePreference::Light
                } else {
                    ThemePreference::Dark
                };
                self.set_theme_preference(next, window, cx);
            }
            PaletteCommand::ReturnToLive => self.return_to_live(window, cx),
            PaletteCommand::SelectRun(id) => {
                self.page = AppPage::Workspace;
                self.select_run(id, window, cx);
            }
        }
        cx.notify();
    }

    fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette_open {
            self.close_command_palette(window, cx);
        } else {
            self.open_command_palette(window, cx);
        }
        cx.notify();
    }

    fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_open = true;
        self.command_highlight = Some(0);
        // The real command input takes keyboard focus so typing filters the
        // palette immediately.
        self.command_input
            .update(cx, |input, cx| input.focus(window, cx));
    }

    fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_open = false;
        self.command_highlight = None;
        self.command_input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn move_command_highlight(&mut self, delta: i32, cx: &App) {
        let count = self.palette_filtered_commands(cx).len();
        if count == 0 {
            self.command_highlight = None;
            return;
        }
        let current = self.command_highlight.unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, count as i32 - 1);
        self.command_highlight = Some(next as usize);
    }

    /// Registers the global ⌘K / Ctrl+K toggle plus the palette's Escape /
    /// Up / Down / Enter keys while it is open (issue 17 item 1).
    fn install_command_interceptor(&mut self, cx: &mut Context<Self>) {
        let app = cx.weak_entity();
        let interceptor = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(app) = app.upgrade() else {
                return;
            };
            let keystroke = &event.keystroke;
            let palette_toggle = keystroke.key.eq_ignore_ascii_case("k")
                && (keystroke.modifiers.platform || keystroke.modifiers.control)
                && !keystroke.modifiers.alt
                && !keystroke.modifiers.shift;
            app.update(cx, |this, cx| {
                if palette_toggle {
                    this.toggle_command_palette(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if !this.command_palette_open {
                    return;
                }
                // Only own the navigation keys while the command input itself
                // has focus; otherwise leave other inputs untouched.
                if !this.command_input.focus_handle(cx).is_focused(window) {
                    return;
                }
                match keystroke.key.as_str() {
                    "escape" => {
                        this.close_command_palette(window, cx);
                        cx.stop_propagation();
                    }
                    "up" => {
                        this.move_command_highlight(-1, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "down" => {
                        this.move_command_highlight(1, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "enter" => {
                        let command = this.command_highlight.and_then(|index| {
                            this.palette_filtered_commands(cx).get(index).copied()
                        });
                        if let Some(command) = command {
                            this.close_command_palette(window, cx);
                            this.execute_palette_command(command, window, cx);
                        }
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            });
        });
        self.command_interceptor = Some(interceptor);
    }

    /// Fraction of the record under the shared epoch cursor.
    fn timeline_cursor_fraction(&self) -> f32 {
        let count = self.display_epochs().len();
        self.selected_epoch
            .filter(|_| count > 1)
            .map(|index| index as f32 / (count - 1) as f32)
            .unwrap_or(0.0)
    }

    /// The visible timeline window `(start, span)` as record fractions. The
    /// cursor is centered in the window unless the seek slider pinned an
    /// offset, which keeps the window still while the pointer drags.
    fn timeline_window(&self) -> (f32, f32) {
        let span = 1.0 / self.timeline_zoom.max(1) as f32;
        let offset = self
            .timeline_cursor_offset
            .map(|offset| offset.clamp(0.0, span))
            .unwrap_or(span / 2.0);
        let start = (self.timeline_cursor_fraction() - offset).clamp(0.0, (1.0 - span).max(0.0));
        (start, span)
    }

    /// Seek: a track fraction of the seek slider maps over the visible
    /// window, and the window stays put so the playhead tracks the pointer.
    fn select_epoch_percent(&mut self, percent: f32) {
        let count = self.display_epochs().len();
        if count == 0 {
            self.selected_epoch = None;
            return;
        }
        let (visible_start, visible_span) = self.timeline_window();
        let track = (percent / 100.0).clamp(0.0, 1.0);
        let global = (visible_start + track * visible_span).clamp(0.0, 1.0);
        self.timeline_cursor_offset = Some(track * visible_span);
        self.selected_epoch = epoch_index_for_percent(count, global * 100.0);
        self.follow_live = global >= 0.9995
            && self.active_run.is_some()
            && self.selected_run_id == self.live_run_id;
    }

    fn step_selected_epoch(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.display_epochs().len();
        if count == 0 {
            return;
        }
        let current = self.selected_epoch.unwrap_or(count - 1);
        let next =
            (current as i64 + delta as i64).clamp(0, count.saturating_sub(1) as i64) as usize;
        self.jump_selected_epoch(next, window, cx);
    }

    fn jump_selected_epoch(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.display_epochs().len();
        if count == 0 {
            return;
        }
        let next = index.min(count - 1);
        self.selected_epoch = Some(next);
        self.follow_live = false;
        self.timeline_cursor_offset = None;
        let percent = if count <= 1 {
            100.0
        } else {
            let (visible_start, visible_span) = self.timeline_window();
            ((self.timeline_cursor_fraction() - visible_start) / visible_span).clamp(0.0, 1.0)
                * 100.0
        };
        self.timeline.update(cx, |timeline, cx| {
            timeline.set_value(percent, window, cx);
        });
    }

    fn adjust_timeline_zoom(&mut self, zoom_in: bool) {
        self.timeline_zoom = match (self.timeline_zoom, zoom_in) {
            (1, true) => 2,
            (2, true) => 4,
            (4, false) => 2,
            (2, false) => 1,
            (zoom, _) => zoom,
        };
        // A zoom change re-centers the cursor in the new window.
        self.timeline_cursor_offset = None;
    }

    /// Open the precise epoch jump popover seeded with the current epoch.
    fn begin_epoch_jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.display_epochs().is_empty() {
            return;
        }
        let number = self.selected_epoch.map(|index| index + 1).unwrap_or(1);
        self.epoch_jump_editing = true;
        self.epoch_jump_input.update(cx, |input, cx| {
            input.set_value(number.to_string(), window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn selected_run(&self) -> Option<&ProcessingRun> {
        self.selected_run_id.and_then(|id| {
            self.active_run
                .as_ref()
                .filter(|run| run.id == id)
                .or_else(|| self.history.iter().find(|run| run.id == id))
        })
    }

    fn settings_issues(&self) -> Vec<GuiSettingsIssue> {
        crate::state::validate_settings(&self.settings.normalized_for_processing())
    }

    fn can_enqueue(&self) -> bool {
        can_enqueue_run(
            self.active_run.is_some(),
            !self.setting_parse_errors.is_empty(),
            !self.settings_issues().is_empty(),
        )
    }

    fn file_label(&self) -> SharedString {
        self.selected_run()
            .map(|run| &run.source_path)
            .or(self.settings.iq_file_path.as_ref())
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                self.localized("未选择数据文件", "No data file selected")
                    .to_string()
            })
            .into()
    }

    fn section_title(title: impl Into<SharedString>, palette: Palette) -> impl IntoElement {
        let title = title.into();
        div().flex().flex_1().items_baseline().min_w_0().child(
            div()
                .flex_shrink_0()
                .text_size(px(13.5))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(title),
        )
    }

    #[allow(dead_code)]
    fn render_legacy_title_bar(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = self.active_run.is_some();
        let page_title = match self.page {
            AppPage::NewAnalysis => self.localized("新建分析", "New analysis"),
            AppPage::Workspace => self.localized("结果工作台", "Result workspace"),
            AppPage::RunLibrary => self.localized("运行库", "Run library"),
            AppPage::InterfaceSettings => self.localized("界面设置", "Interface settings"),
        };

        TitleBar::new()
            .bg(palette.surface)
            .border_color(palette.border)
            .text_color(palette.text)
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .flex_shrink_0()
                            .text_color(palette.accent)
                            .child(IconName::Map)
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(palette.text)
                                    .child("GNSScope"),
                            ),
                    )
                    .children(
                        matches!(self.page, AppPage::NewAnalysis | AppPage::Workspace).then(|| {
                            div()
                                .h(px(18.))
                                .w(px(1.))
                                .flex_shrink_0()
                                .bg(palette.border)
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(page_title),
                    )
                    .children((!compact && self.page == AppPage::Workspace).then(|| {
                        div()
                            .max_w(px(360.))
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(9.))
                            .text_color(palette.muted)
                            .child(format!("·  {}", self.file_label()))
                    }))
                    .child(div().flex_1())
                    .children(is_active.then(|| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(4.))
                            .bg(palette.accent.opacity(0.10))
                            .text_size(px(9.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.accent)
                            .child(div().size(px(6.)).rounded_full().bg(palette.accent))
                            .child(self.localized("正在处理", "Processing"))
                    }))
                    .children((self.page == AppPage::NewAnalysis).then(|| {
                        GnssButton::new(
                            "title-select-file",
                            self.localized("选择 IQ", "Choose IQ"),
                            palette,
                        )
                        .kind(ButtonKind::Ghost)
                        .size(ControlSize::Compact)
                        .icon(IconName::FolderOpen)
                        .tooltip(self.localized("选择 IQ 文件", "Select IQ file"))
                        .on_click(cx.listener(|this, _, _, cx| this.select_file(cx)))
                    }))
                    .child(
                        div()
                            .h(px(18.))
                            .w(px(1.))
                            .flex_shrink_0()
                            .bg(palette.border),
                    )
                    .children(
                        (self.page == AppPage::Workspace && is_active && !self.follow_live).then(
                            || {
                                GnssButton::icon_only(
                                    "return-live",
                                    IconName::Redo2,
                                    self.localized("返回最新 epoch", "Return to latest epoch"),
                                    palette,
                                )
                                .size(ControlSize::Compact)
                                .tooltip(self.localized("返回最新 epoch", "Return to latest epoch"))
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.return_to_live(window, cx);
                                        cx.notify();
                                    },
                                ))
                            },
                        ),
                    )
                    .children((self.page == AppPage::Workspace).then(|| {
                        GnssButton::icon_only(
                            "settings",
                            IconName::Settings2,
                            self.localized("接收机详细配置", "Receiver processing settings"),
                            palette,
                        )
                        .size(ControlSize::Compact)
                        .tooltip(self.localized("接收机详细配置", "Receiver processing settings"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            let side_drawer =
                                toggled_side_drawer(this.side_drawer, SideDrawer::Settings);
                            if side_drawer == Some(SideDrawer::Settings) {
                                this.sync_setting_inputs(window, cx);
                            }
                            this.side_drawer = side_drawer;
                            cx.notify();
                        }))
                    }))
                    .children((self.page == AppPage::NewAnalysis && !is_active).then(|| {
                        GnssButton::new(
                            "start-analysis",
                            self.localized("开始处理", "Start processing"),
                            palette,
                        )
                        .kind(ButtonKind::Primary)
                        .size(ControlSize::Compact)
                        .icon(IconName::SquareTerminal)
                        .disabled(!self.can_enqueue())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.enqueue_selected_or_draft(cx);
                            if this.active_run.is_some() {
                                this.page = AppPage::Workspace;
                            }
                            cx.notify();
                        }))
                    }))
                    .children((self.page == AppPage::Workspace).then(|| {
                        if is_active {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(
                                    GnssButton::icon_only(
                                        "enqueue",
                                        IconName::Plus,
                                        self.localized("加入等待队列", "Add to queue"),
                                        palette,
                                    )
                                    .kind(ButtonKind::Primary)
                                    .size(ControlSize::Compact)
                                    .disabled(!self.can_enqueue())
                                    .on_click(cx.listener(
                                        |this, _, _, cx| this.enqueue_selected_or_draft(cx),
                                    )),
                                )
                                .child(
                                    GnssButton::icon_only(
                                        "cancel",
                                        IconName::CircleX,
                                        self.localized("取消当前解算", "Cancel current solution"),
                                        palette,
                                    )
                                    .kind(ButtonKind::QuietDestructive)
                                    .size(ControlSize::Compact)
                                    .tooltip(
                                        self.localized("取消当前解算", "Cancel current solution"),
                                    )
                                    .on_click(cx.listener(|this, _, _, _| this.cancel_active())),
                                )
                                .into_any_element()
                        } else if self.selected_run_id.is_some() {
                            GnssButton::icon_only(
                                "run",
                                IconName::Replace,
                                self.localized(
                                    "按此配置重新解算",
                                    "Reprocess with this configuration",
                                ),
                                palette,
                            )
                            .kind(ButtonKind::Primary)
                            .size(ControlSize::Compact)
                            .disabled(!self.can_enqueue())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.enqueue_selected_or_draft(cx)),
                            )
                            .into_any_element()
                        } else {
                            GnssButton::new(
                                "workspace-new-analysis",
                                self.localized("新建分析", "New analysis"),
                                palette,
                            )
                            .kind(ButtonKind::Primary)
                            .size(ControlSize::Compact)
                            .icon(IconName::Plus)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_new_task(window, cx);
                                cx.notify();
                            }))
                            .into_any_element()
                        }
                    })),
            )
    }

    fn render_title_bar(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected_name = self.file_label();
        let page_context = match self.page {
            AppPage::NewAnalysis => self.localized("分析草稿", "Analysis draft"),
            AppPage::InterfaceSettings => self.localized("界面设置", "Interface settings"),
            AppPage::RunLibrary => self.localized("运行库", "Run library"),
            AppPage::Workspace => self.localized("选中的运行", "Selected run"),
        };

        let settings_control = GnssButton::new(
            "global-interface-settings",
            self.localized("设置", "Settings"),
            palette,
        )
        .kind(ButtonKind::Ghost)
        .size(ControlSize::Compact)
        .selected(self.page == AppPage::InterfaceSettings)
        .icon(IconName::Settings2)
        .tooltip(self.localized("打开界面设置", "Open interface settings"))
        .on_click(cx.listener(|this, _, _, cx| {
            this.page = AppPage::InterfaceSettings;
            this.compact_library_expanded = false;
            this.side_drawer = None;
            this.focused_plot = None;
            cx.notify();
        }));
        // The title-bar field is the real command input (issue 17 item 1): it
        // owns keyboard focus and filters the ⌘K palette. Chrome (icon, ring,
        // size) is title_bar's `command_field`'s job; keep the input itself
        // transparent so the field styling shows through.
        let command_input = Input::new(&self.command_input)
            .w_full()
            .bg(gpui_kit::transparent_black())
            .border_color(gpui_kit::transparent_black())
            .text_size(px(11.))
            .text_color(palette.text);
        // Workspace title-bar run selector (spec §3.1): a bordered ~240px
        // field carrying the middle-ellipsized data-font filename and a ▾
        // affordance; the menu lists the active run plus history with a
        // localized status word (non-color cue) and a checked mark on the
        // selected run. Selecting an entry switches the inspected run; the
        // active processing run keeps running regardless.
        let run_selector = (self.page == AppPage::Workspace).then(|| {
            let full_name = self.file_label().to_string();
            let selector_label = crate::ui::primitives::middle_ellipsis(&full_name, 28);
            let app = cx.entity();
            let selected_id = self.selected_run_id;
            let items: Vec<(uuid::Uuid, String, bool)> = self
                .active_run
                .iter()
                .chain(self.history.iter())
                .take(12)
                .map(|run| {
                    let status = match run.status {
                        RunStatus::Completed => self.localized("已完成", "Completed"),
                        RunStatus::Failed => self.localized("失败", "Failed"),
                        RunStatus::Cancelled => self.localized("已取消", "Cancelled"),
                        RunStatus::Interrupted => self.localized("已中断", "Interrupted"),
                        RunStatus::Queued => self.localized("排队", "Queued"),
                        _ => self.localized("处理中", "Processing"),
                    };
                    let name = run
                        .source_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let name = crate::ui::primitives::middle_ellipsis(&name, 30);
                    (
                        run.id,
                        format!("{status} · {name}"),
                        selected_id == Some(run.id),
                    )
                })
                .collect();
            let empty = items.is_empty();
            GnssButton::new("title-bar-run-selector", selector_label, palette)
                .kind(ButtonKind::Ghost)
                .size(ControlSize::Compact)
                .trailing_icon(IconName::ChevronDown)
                .tooltip(full_name)
                .w(px(268.))
                .disabled(empty)
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu.min_w(px(240.)).max_w(px(320.));
                    for (id, label, selected) in &items {
                        let id = *id;
                        let selected = *selected;
                        let app = app.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label.clone())
                                .checked(selected)
                                .on_click(move |_, window, cx| {
                                    app.update(cx, |this, cx| {
                                        this.page = AppPage::Workspace;
                                        this.select_run(id, window, cx);
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                })
                .into_any_element()
        });
        crate::ui::title_bar::render(
            selected_name,
            page_context,
            run_selector,
            compact,
            command_input.into_any_element(),
            settings_control.into_any_element(),
            palette,
        )
    }

    #[allow(dead_code)]
    fn render_sidebar(
        &self,
        palette: Palette,
        stacked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let history_query = self.history_search.read(cx).value().to_string();
        if stacked && !self.compact_library_expanded {
            let run_label = self
                .selected_run()
                .and_then(|run| run.source_path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.localized("未选择任务", "No task selected").to_string());
            return div()
                .w_full()
                .h(px(50.))
                .flex_shrink_0()
                .px(px(8.))
                .bg(palette.sidebar)
                .border_b_1()
                .border_color(palette.border)
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    Button::new("compact-new-task")
                        .ghost()
                        .icon(IconName::Plus)
                        .tooltip(self.localized(
                            "选择 IQ 文件并创建新的解算任务",
                            "Select an IQ file and create a new processing task",
                        ))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_new_task(window, cx);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(px(6.))
                        .child(
                            div()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_size(px(11.))
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(palette.text)
                                .child(run_label),
                        )
                        .child(div().text_size(px(9.)).text_color(palette.muted).child(
                            match self.interface_language {
                                InterfaceLanguage::SimplifiedChinese => format!(
                                    "{} 条历史 · {} 项排队",
                                    self.history.len(),
                                    self.queue.len()
                                ),
                                InterfaceLanguage::English => format!(
                                    "{} history · {} queued",
                                    self.history.len(),
                                    self.queue.len()
                                ),
                            },
                        )),
                )
                .child(
                    Button::new("compact-expand-library")
                        .ghost()
                        .icon(IconName::PanelBottomOpen)
                        .tooltip(self.localized(
                            "展开运行库、等待队列和历史记录",
                            "Expand run library, queue, and history",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.compact_library_expanded = true;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("compact-interface-settings")
                        .ghost()
                        .icon(IconName::Settings2)
                        .tooltip(self.localized(
                            "打开界面外观与显示设置",
                            "Open appearance and display settings",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.page = AppPage::InterfaceSettings;
                            this.side_drawer = None;
                            this.focused_plot = None;
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        let active = self.active_run.as_ref().map(|run| {
            div()
                .p(px(10.))
                .rounded(px(6.))
                .bg(palette.surface)
                .border_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child(
                                    run.source_path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned(),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.accent)
                                .child(format!("{:.0}%", run.progress * 100.0)),
                        ),
                )
                .child(
                    Progress::new("run-progress")
                        .mt(px(8.))
                        .h(px(4.))
                        .bg(palette.accent)
                        .value((run.progress * 100.0) as f32),
                )
        });
        div()
            .when(!stacked, |this| {
                this.w(px(236.)).min_w(px(216.)).h_full().border_r_1()
            })
            .when(stacked, |this| {
                this.w_full().h(px(190.)).flex_shrink_0().border_b_1()
            })
            .bg(palette.sidebar)
            .border_color(palette.border)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p(px(12.))
                    .child(
                        Button::new("new-task")
                            .w_full()
                            .ghost()
                            .rounded(px(4.))
                            .bg(palette.surface_alt)
                            .justify_start()
                            .text_color(palette.text)
                            .icon(IconName::Plus)
                            .label(self.localized("新建任务", "New task"))
                            .tooltip(self.localized(
                                "选择 IQ 文件并创建新的解算任务",
                                "Select an IQ file and create a new processing task",
                            ))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_new_task(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .mt(px(14.))
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .mb(px(7.))
                            .child(self.localized("当前任务", "Current task")),
                    )
                    .children(active)
                    .when(self.active_run.is_none(), |this| {
                        this.child(
                            div()
                                .p(px(10.))
                                .text_size(px(11.))
                                .text_color(palette.muted)
                                .child(self.localized("暂无运行中的任务", "No active task")),
                        )
                    })
                    .child(
                        div()
                            .mt(px(18.))
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .mb(px(7.))
                            .child(match self.interface_language {
                                InterfaceLanguage::SimplifiedChinese => {
                                    format!("等待队列  {}", self.queue.len())
                                }
                                InterfaceLanguage::English => {
                                    format!("Queue  {}", self.queue.len())
                                }
                            }),
                    )
                    .children(self.queue.iter().enumerate().map(|(index, run)| {
                        let id = run.id;
                        div()
                            .mb(px(5.))
                            .px(px(9.))
                            .py(px(6.))
                            .rounded(px(5.))
                            .bg(palette.surface_alt)
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .max_w(px(145.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_size(px(11.))
                                    .child(
                                        run.source_path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned(),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .child(
                                        Button::new(SharedString::from(format!("queue-up-{id}")))
                                            .ghost()
                                            .icon(IconName::ChevronUp)
                                            .tooltip(self.localized("上移", "Move up"))
                                            .disabled(index == 0)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.move_queued(id, -1);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("queue-down-{id}")))
                                            .ghost()
                                            .icon(IconName::ChevronDown)
                                            .tooltip(self.localized("下移", "Move down"))
                                            .disabled(index + 1 == self.queue.len())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.move_queued(id, 1);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "cancel-queued-{id}"
                                        )))
                                        .ghost()
                                        .icon(IconName::CircleX)
                                        .tooltip(self.localized("取消排队", "Remove from queue"))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.cancel_queued(id);
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            )
                    }))
                    .child(
                        div()
                            .mt(px(18.))
                            .mb(px(7.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(palette.muted)
                                    .child(self.localized("历史记录", "History")),
                            )
                            .child(
                                Button::new("history-filter")
                                    .ghost()
                                    .icon(IconName::SortDescending)
                                    .label(match (self.interface_language, self.history_filter) {
                                        (InterfaceLanguage::SimplifiedChinese, filter) => {
                                            filter.label(self.interface_language)
                                        }
                                        (InterfaceLanguage::English, HistoryFilter::All) => "All",
                                        (InterfaceLanguage::English, HistoryFilter::Completed) => {
                                            "Completed"
                                        }
                                        (
                                            InterfaceLanguage::English,
                                            HistoryFilter::NeedsAttention,
                                        ) => "Needs attention",
                                    })
                                    .tooltip(self.localized(
                                        "切换历史状态过滤",
                                        "Cycle history status filter",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.history_filter = this.history_filter.next();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div().mb(px(8.)).h(px(32.)).child(
                            Input::new(&self.history_search)
                                .prefix(IconName::Search)
                                .bg(palette.background),
                        ),
                    )
                    .children(
                        self.history
                            .iter()
                            .filter(|run| {
                                run.status != RunStatus::Queued
                                    && Some(run.id)
                                        != self.active_run.as_ref().map(|active| active.id)
                                    && history_run_matches(run, &history_query, self.history_filter)
                            })
                            .take(30)
                            .map(|run| {
                                let id = run.id;
                                let selected = self.selected_run_id == Some(id);
                                div()
                                    .id(SharedString::from(format!("run-{id}")))
                                    .mb(px(5.))
                                    .px(px(9.))
                                    .py(px(8.))
                                    .rounded(px(5.))
                                    .bg(if selected {
                                        palette.surface
                                    } else {
                                        palette.surface_alt
                                    })
                                    .border_1()
                                    .border_color(if selected {
                                        palette.accent
                                    } else {
                                        palette.border
                                    })
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.page = AppPage::Workspace;
                                        this.select_run(id, window, cx);
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .max_w(px(150.))
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .text_size(px(11.))
                                                    .text_color(palette.text)
                                                    .child(
                                                        run.source_path
                                                            .file_name()
                                                            .unwrap_or_default()
                                                            .to_string_lossy()
                                                            .into_owned(),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(9.))
                                                    .text_color(match run.status {
                                                        RunStatus::Completed => palette.healthy,
                                                        RunStatus::Failed => palette.danger,
                                                        _ => palette.warning,
                                                    })
                                                    .child(format!("{:?}", run.status)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.))
                                            .text_size(px(9.))
                                            .text_color(palette.muted)
                                            .child(format!(
                                                "{} epochs · {:.0}%",
                                                run.pvt_epoch_count,
                                                run.progress * 100.0
                                            )),
                                    )
                                    .context_menu(self.history_context_menu(id, cx))
                            }),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(palette.border)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        Button::new("interface-settings-page")
                            .flex_1()
                            .ghost()
                            .justify_start()
                            .text_color(if self.page == AppPage::InterfaceSettings {
                                palette.accent
                            } else {
                                palette.text
                            })
                            .bg(palette.sidebar)
                            .icon(IconName::Settings2)
                            .label(self.localized("界面设置", "Interface settings"))
                            .tooltip(self.localized("打开界面设置", "Open interface settings"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.page = AppPage::InterfaceSettings;
                                this.side_drawer = None;
                                this.focused_plot = None;
                                cx.notify();
                            })),
                    )
                    .children(stacked.then(|| {
                        Button::new("compact-collapse-library")
                            .ghost()
                            .icon(IconName::PanelBottom)
                            .tooltip(self.localized(
                                "收起运行库并恢复工作区高度",
                                "Collapse the run library and restore workspace height",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.compact_library_expanded = false;
                                cx.notify();
                            }))
                    })),
            )
            .into_any_element()
    }

    fn runs_row_view(
        &self,
        run: &ProcessingRun,
        palette: Palette,
    ) -> crate::ui::run_rows::RunRowView {
        // Source-file availability (issue 17 item 5): a persisted run whose
        // source no longer exists still browses, but its row says 源不可用.
        // Preview fixtures never touch the filesystem — their story paths are
        // deterministic display data, so the check is skipped under
        // GNSSCOPE_PREVIEW=1 (data-honesty: preview story only in preview).
        let source_metadata = if self.preview_mode {
            None
        } else {
            std::fs::metadata(&run.source_path).ok()
        };
        let source_missing = !self.preview_mode
            && source_metadata.is_none()
            && !matches!(run.status, RunStatus::Queued);
        let (status, status_color) = if source_missing {
            (
                self.localized("源不可用", "Source unavailable"),
                palette.warning,
            )
        } else {
            match run.status {
                RunStatus::Completed => (self.localized("已完成", "Completed"), palette.healthy),
                RunStatus::Failed => (self.localized("失败", "Failed"), palette.danger),
                RunStatus::Cancelled => (self.localized("已取消", "Cancelled"), palette.warning),
                RunStatus::Interrupted => {
                    (self.localized("已中断", "Interrupted"), palette.warning)
                }
                RunStatus::Queued => (self.localized("排队", "Queued"), palette.attention),
                _ => (self.localized("处理中", "Processing"), palette.accent),
            }
        };
        crate::ui::run_rows::RunRowView {
            id: run.id,
            selected: self.selected_run_id == Some(run.id),
            filename: run
                .source_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            created_short: run.created_at.format("%H:%M:%S").to_string(),
            created_long: run.created_at.format("%Y-%m-%d  %H:%M:%S").to_string(),
            elapsed: format_timeline_time(processing_run_elapsed_ms(run)),
            progress: run.progress,
            pvt_epoch_count: run.pvt_epoch_count,
            samples_total: (run.total_samples > 0).then_some(run.total_samples),
            size_bytes: source_metadata.as_ref().map(|metadata| metadata.len()),
            source_path: Some(run.source_path.to_string_lossy().into_owned()),
            status,
            status_color,
            attention: matches!(run.status, RunStatus::Cancelled | RunStatus::Interrupted),
        }
    }
    fn render_runs_dock(
        &self,
        palette: Palette,
        full_width: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let history_query = self.history_search.read(cx).value().to_string();
        let filter_menu_app = cx.entity();
        let current_filter = self.history_filter;
        let interface_language = self.interface_language;
        let localize: crate::ui::Localize = match self.interface_language {
            InterfaceLanguage::SimplifiedChinese => |zh, _| zh,
            InterfaceLanguage::English => |_, en| en,
        };
        let app = cx.entity();
        let actions: crate::ui::run_rows::RunHandler =
            std::rc::Rc::new(move |action, window, cx| {
                app.update(cx, |this, cx| {
                    use crate::ui::run_rows::RunAction;
                    match action {
                        RunAction::Select(id) => {
                            this.page = AppPage::Workspace;
                            this.select_run(id, window, cx);
                        }
                        RunAction::ContextSelect(id) => this.select_context_run(id, window, cx),
                        RunAction::MoveUp(id) => this.move_queued(id, -1),
                        RunAction::MoveDown(id) => this.move_queued(id, 1),
                        RunAction::Cancel(id) => this.cancel_queued(id),
                        RunAction::Reveal(id) => this.reveal_source_file(id),
                        RunAction::Delete(id) => this.request_delete_historical_run(id, window, cx),
                    }
                    cx.notify();
                });
            });
        let active = self.active_run.as_ref().map(|run| {
            crate::ui::run_rows::active_row(
                self.runs_row_view(run, palette),
                actions.clone(),
                localize,
                palette,
            )
        });
        let active_rows = active
            .map(|row| row.into_any_element())
            .into_iter()
            .collect();
        let queue_rows = (self.queue.iter().enumerate().map(|(index, run)| {
            crate::ui::run_rows::queue_row(
                self.runs_row_view(run, palette),
                index,
                self.queue.len(),
                actions.clone(),
                localize,
                palette,
            )
        }))
        .map(|row| row.into_any_element())
        .collect();
        let history_window = dock_history_window(
            &self.history,
            self.active_run.as_ref().map(|run| run.id),
            &history_query,
            self.history_filter,
        );
        let shown_history = history_window.rows.len();
        let matching_history = history_window.matching;
        let history_rows = (history_window.rows.into_iter().map(|run| {
            crate::ui::run_rows::history_row(
                self.runs_row_view(run, palette),
                actions.clone(),
                localize,
                palette,
            )
        }))
        .map(|row| row.into_any_element())
        .collect();
        let header_actions = div()
            .flex()
            .items_center()
            .child(
                GnssButton::icon_only(
                    "runs-new-analysis",
                    IconName::Plus,
                    self.localized("新建分析 · ⌘N", "New analysis · Cmd+N"),
                    palette,
                )
                .size(ControlSize::Compact)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.start_new_task(window, cx);
                    cx.notify();
                })),
            )
            .child(
                GnssButton::icon_only(
                    "runs-filter-menu",
                    IconName::Ellipsis,
                    self.localized("选择运行筛选", "Choose run filter"),
                    palette,
                )
                .size(ControlSize::Compact)
                .selected(current_filter != HistoryFilter::All)
                .dropdown_menu(move |menu, _, _| {
                    menu.min_w(px(172.))
                        .item(Self::history_filter_menu_item(
                            filter_menu_app.clone(),
                            HistoryFilter::All,
                            current_filter,
                            interface_language,
                        ))
                        .item(Self::history_filter_menu_item(
                            filter_menu_app.clone(),
                            HistoryFilter::Completed,
                            current_filter,
                            interface_language,
                        ))
                        .item(Self::history_filter_menu_item(
                            filter_menu_app.clone(),
                            HistoryFilter::NeedsAttention,
                            current_filter,
                            interface_language,
                        ))
                }),
            )
            .into_any_element();
        let footer = match self.interface_language {
            InterfaceLanguage::SimplifiedChinese => format!(
                "显示 {shown_history}/{matching_history} 条历史{}",
                if matching_history > shown_history {
                    " · 请细化搜索"
                } else {
                    ""
                }
            ),
            InterfaceLanguage::English => {
                format!(
                    "History {shown_history}/{matching_history}{}",
                    if matching_history > shown_history {
                        " · Refine search"
                    } else {
                        ""
                    }
                )
            }
        };
        crate::ui::runs_dock::render(
            crate::ui::runs_dock::RunsView {
                title: self.localized("运行", "Runs"),
                full_width,
                sections: [
                    crate::ui::runs_dock::RunSection {
                        title: self.localized("当前运行", "Active"),
                        count: usize::from(self.active_run.is_some()),
                        rows: active_rows,
                        empty: self
                            .active_run
                            .is_none()
                            .then_some(self.localized("没有正在处理的运行", "No active run")),
                    },
                    crate::ui::runs_dock::RunSection {
                        title: self.localized("等待队列", "Queue"),
                        count: self.queue.len(),
                        rows: queue_rows,
                        empty: None,
                    },
                    crate::ui::runs_dock::RunSection {
                        title: self.localized("历史记录", "History"),
                        count: matching_history,
                        rows: history_rows,
                        empty: (matching_history == 0).then_some(self.localized(
                            "没有匹配的历史运行；请调整搜索或筛选条件。",
                            "No matching history. Adjust search or filters.",
                        )),
                    },
                ],
                footer,
            },
            header_actions,
            gnss_search_input(&self.history_search, palette).into_any_element(),
            palette,
        )
    }

    #[allow(dead_code)]
    fn render_app_navigation(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if compact {
            return div()
                .w_full()
                .h(px(58.))
                .flex_shrink_0()
                .px(px(8.))
                .bg(palette.sidebar)
                .border_b_1()
                .border_color(palette.border)
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    Button::new("compact-new-analysis")
                        .primary()
                        .icon(IconName::Plus)
                        .tooltip(self.localized("新建分析", "New analysis"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_new_task(window, cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("compact-workspace")
                        .ghost()
                        .icon(IconName::LayoutDashboard)
                        .text_color(if self.page == AppPage::Workspace {
                            palette.accent
                        } else {
                            palette.muted
                        })
                        .bg(if self.page == AppPage::Workspace {
                            palette.accent.opacity(0.10)
                        } else {
                            palette.sidebar
                        })
                        .tooltip(self.localized("结果工作台", "Result workspace"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.page = AppPage::Workspace;
                            this.side_drawer = None;
                            this.focused_plot = None;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("compact-run-library")
                        .ghost()
                        .icon(IconName::ChartPie)
                        .text_color(if self.page == AppPage::RunLibrary {
                            palette.accent
                        } else {
                            palette.muted
                        })
                        .bg(if self.page == AppPage::RunLibrary {
                            palette.accent.opacity(0.10)
                        } else {
                            palette.sidebar
                        })
                        .tooltip(self.localized("运行库", "Run library"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.page = AppPage::RunLibrary;
                            this.side_drawer = None;
                            this.focused_plot = None;
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .children(self.active_run.as_ref().map(|run| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .text_size(px(9.))
                        .font_family(data_font_family())
                        .text_color(palette.accent)
                        .child(div().size(px(6.)).rounded_full().bg(palette.accent))
                        .child(format!("{:.0}%", run.progress * 100.0))
                }))
                .child(
                    Button::new("compact-interface-settings-v2")
                        .ghost()
                        .icon(IconName::Settings2)
                        .text_color(if self.page == AppPage::InterfaceSettings {
                            palette.accent
                        } else {
                            palette.muted
                        })
                        .tooltip(self.localized("界面设置", "Interface settings"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.page = AppPage::InterfaceSettings;
                            this.side_drawer = None;
                            this.focused_plot = None;
                            cx.notify();
                        })),
                )
                .into_any_element();
        }

        let nav_button =
            |id: &'static str, icon: IconName, label: &'static str, active: bool, page: AppPage| {
                Button::new(id)
                    .w_full()
                    .ghost()
                    .justify_start()
                    .rounded(px(5.))
                    .icon(icon)
                    .label(label)
                    .text_color(if active {
                        palette.accent
                    } else {
                        palette.muted
                    })
                    .bg(if active {
                        palette.accent.opacity(0.10)
                    } else {
                        palette.sidebar
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = page;
                        this.side_drawer = None;
                        this.focused_plot = None;
                        cx.notify();
                    }))
            };

        div()
            .w(px(224.))
            .min_w(px(224.))
            .h_full()
            .bg(palette.sidebar)
            .border_r_1()
            .border_color(palette.border)
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(12.))
                    .pt(px(14.))
                    .pb(px(10.))
                    .child(
                        Button::new("new-analysis")
                            .w_full()
                            .primary()
                            .justify_start()
                            .rounded(px(5.))
                            .bg(palette.accent)
                            .text_color(palette.on_accent)
                            .icon(IconName::Plus)
                            .label(self.localized("新建分析", "New analysis"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_new_task(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .mt(px(14.))
                            .mb(px(5.))
                            .px(px(8.))
                            .text_size(px(9.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.muted)
                            .child(self.localized("工作区", "WORKSPACE")),
                    )
                    .child(nav_button(
                        "nav-workspace",
                        IconName::LayoutDashboard,
                        self.localized("结果工作台", "Result workspace"),
                        self.page == AppPage::Workspace,
                        AppPage::Workspace,
                    ))
                    .child(nav_button(
                        "nav-run-library",
                        IconName::ChartPie,
                        self.localized("运行库", "Run library"),
                        self.page == AppPage::RunLibrary,
                        AppPage::RunLibrary,
                    )),
            )
            .children(self.active_run.as_ref().map(|run| {
                div()
                    .mx(px(12.))
                    .mt(px(8.))
                    .p(px(10.))
                    .rounded(px(6.))
                    .bg(palette.surface)
                    .border_1()
                    .border_color(palette.accent.opacity(0.45))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .text_size(px(9.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.accent)
                            .child(div().size(px(6.)).rounded_full().bg(palette.accent))
                            .child(self.localized("正在处理", "PROCESSING")),
                    )
                    .child(
                        div()
                            .mt(px(7.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(11.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(
                                run.source_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(7.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div().flex_1().child(
                                    Progress::new("queue-progress")
                                        .h(px(3.))
                                        .bg(palette.accent)
                                        .value((run.progress * 100.0) as f32),
                                ),
                            )
                            .child(
                                div()
                                    .text_size(px(9.))
                                    .font_family(data_font_family())
                                    .text_color(palette.accent)
                                    .child(format!("{:.0}%", run.progress * 100.0)),
                            ),
                    )
            }))
            .child(div().flex_1())
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(palette.border)
                    .child(nav_button(
                        "nav-interface-settings",
                        IconName::Settings2,
                        self.localized("界面设置", "Interface settings"),
                        self.page == AppPage::InterfaceSettings,
                        AppPage::InterfaceSettings,
                    ))
                    .child(
                        div()
                            .mt(px(8.))
                            .px(px(8.))
                            .text_size(px(8.))
                            .text_color(palette.muted)
                            .child(match self.interface_language {
                                InterfaceLanguage::SimplifiedChinese => {
                                    format!(
                                        "{} 条记录 · {} 项排队",
                                        self.history.len(),
                                        self.queue.len()
                                    )
                                }
                                InterfaceLanguage::English => {
                                    format!(
                                        "{} runs · {} queued",
                                        self.history.len(),
                                        self.queue.len()
                                    )
                                }
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_new_analysis_content(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let issue_count = self.settings_issues().len() + self.setting_parse_errors.len();
        let source_path = self
            .settings
            .iq_file_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let category = self.settings_category;
        let source_ready = source_path.is_some();

        div()
            .w_full()
            .child(
                div()
                    .w_full()
                    .max_w(px(1120.))
                    .mx_auto()
                    .px(if compact { px(16.) } else { px(28.) })
                    .py(px(24.))
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                    .text_color(palette.text)
                                    .child(self.localized(
                                        "创建离线信号分析",
                                        "Create an offline signal analysis",
                                    )),
                            )
                            .child(
                                div()
                                    .mt(px(5.))
                                    .text_size(px(11.))
                                    .text_color(palette.muted)
                                    .child(self.localized(
                                        "选择记录文件，确认接收机参数，然后启动确定性的单次处理。",
                                        "Choose a recording, confirm receiver parameters, then start a deterministic single-pass run.",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(18.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .children([
                                ("1", self.localized("数据源", "Source"), source_ready),
                                (
                                    "2",
                                    self.localized("接收机", "Receiver"),
                                    source_ready && issue_count == 0,
                                ),
                                (
                                    "3",
                                    self.localized("开始处理", "Start"),
                                    source_ready && issue_count == 0,
                                ),
                            ]
                            .into_iter()
                            .map(|(number, label, complete)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .size(px(22.))
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .bg(if complete {
                                                palette.accent
                                            } else {
                                                palette.surface_alt
                                            })
                                            .border_1()
                                            .border_color(if complete {
                                                palette.accent
                                            } else {
                                                palette.border
                                            })
                                            .text_size(px(9.))
                                            .font_family(data_font_family())
                                            .text_color(if complete {
                                                palette.on_accent
                                            } else {
                                                palette.muted
                                            })
                                            .child(number),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(if complete {
                                                palette.text
                                            } else {
                                                palette.muted
                                            })
                                            .child(label),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .mt(px(20.))
                            .flex()
                            .when(compact, |this| this.flex_col())
                            .gap(px(14.))
                            .child(
                                div()
                                    .when(compact, |this| this.w_full())
                                    .when(!compact, |this| this.w(px(300.)))
                                    .flex_shrink_0()
                                    .child(
                                        div()
                                            .p(px(16.))
                                            .rounded(px(7.))
                                            .bg(palette.surface)
                                            .border_1()
                                            .border_color(if source_ready {
                                                palette.accent.opacity(0.50)
                                            } else {
                                                palette.border
                                            })
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(Self::section_title(
                                                        self.localized("IQ 数据源", "IQ source"),
                                                        palette,
                                                    ))
                                                    .child(
                                                        div()
                                                            .size(px(30.))
                                                            .rounded(px(5.))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .bg(palette.accent.opacity(0.10))
                                                            .text_color(palette.accent)
                                                            .child(IconName::FolderOpen),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(14.))
                                                    .p(px(12.))
                                                    .rounded(px(5.))
                                                    .bg(palette.background)
                                                    .border_1()
                                                    .border_color(palette.border)
                                                    .child(
                                                        div()
                                                            .overflow_hidden()
                                                            .text_ellipsis()
                                                            .text_size(px(11.))
                                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                            .text_color(if source_ready {
                                                                palette.text
                                                            } else {
                                                                palette.muted
                                                            })
                                                            .child(self.file_label()),
                                                    )
                                                    .children(source_path.map(|path| {
                                                        div()
                                                            .mt(px(4.))
                                                            .overflow_hidden()
                                                            .text_ellipsis()
                                                            .text_size(px(8.))
                                                            .text_color(palette.muted)
                                                            .child(path)
                                                    })),
                                            )
                                            .child(
                                                GnssButton::new(
                                                    "new-analysis-choose-source",
                                                    if source_ready {
                                                        self.localized("更换文件", "Replace file")
                                                    } else {
                                                        self.localized(
                                                            "选择 IQ 文件",
                                                            "Choose IQ file",
                                                        )
                                                    },
                                                    palette,
                                                )
                                                    .mt(px(12.))
                                                    .w_full()
                                                    .icon(IconName::FolderOpen)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.select_file(cx)
                                                    })),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(12.))
                                            .p(px(16.))
                                            .rounded(px(7.))
                                            .bg(palette.surface)
                                            .border_1()
                                            .border_color(palette.border)
                                            .child(Self::section_title(
                                                self.localized("信号方案", "Signal profile"),
                                                palette,
                                            ))
                                            .child(
                                                div()
                                                    .mt(px(12.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                            .child("GPS L1 C/A"),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(7.))
                                                            .py(px(3.))
                                                            .rounded(px(3.))
                                                            .bg(palette.healthy.opacity(0.10))
                                                            .text_size(px(8.))
                                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                            .text_color(palette.healthy)
                                                            .child(self.localized("可用", "AVAILABLE")),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(8.))
                                                    .text_size(px(9.))
                                                    .text_color(palette.muted)
                                                    .child(self.localized(
                                                        "其他系统与频点将在统一信号抽象完成后加入。",
                                                        "Additional constellations and bands will follow the unified signal abstraction.",
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .rounded(px(7.))
                                    .bg(palette.surface)
                                    .border_1()
                                    .border_color(palette.border)
                                    .child(
                                        div()
                                            .px(px(16.))
                                            .py(px(13.))
                                            .border_b_1()
                                            .border_color(palette.border)
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(Self::section_title(
                                                self.localized(
                                                    "接收机配置",
                                                    "Receiver configuration",
                                                ),
                                                palette,
                                            ))
                                            .child(
                                                GnssButton::new(
                                                    "new-analysis-reset",
                                                    self.localized("恢复默认", "Reset"),
                                                    palette,
                                                )
                                                    .kind(ButtonKind::Ghost)
                                                    .icon(IconName::Undo2)
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.reset_receiver_settings(window, cx);
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    )
                                    .children((issue_count > 0).then(|| {
                                        // A quiet inline notice with a 2px danger
                                        // rule, not a full-width tinted band
                                        // (design pass: no loud banners).
                                        div()
                                            .mx(px(14.))
                                            .mt(px(12.))
                                            .pl(px(9.))
                                            .py(px(3.))
                                            .border_l_2()
                                            .border_color(palette.danger)
                                            .flex()
                                            .items_center()
                                            .gap(px(7.))
                                            .text_size(px(11.))
                                            .text_color(palette.danger)
                                            .child(IconName::TriangleAlert)
                                            .child(match self.interface_language {
                                                InterfaceLanguage::SimplifiedChinese => {
                                                    format!("{issue_count} 项配置需要处理")
                                                }
                                                InterfaceLanguage::English => {
                                                    format!("{issue_count} configuration issues")
                                                }
                                            })
                                    }))
                                    .child(
                                        div()
                                            .px(px(10.))
                                            .py(px(8.))
                                            .flex()
                                            .flex_wrap()
                                            .gap(px(4.))
                                            .border_b_1()
                                            .border_color(palette.border)
                                            .children(SettingsCategory::ALL.into_iter().map(
                                                |candidate| {
                                                    let selected = category == candidate;
                                                    GnssButton::new(
                                                        SharedString::from(format!(
                                                            "new-analysis-category-{:?}",
                                                            candidate
                                                        )),
                                                        candidate.label(self.interface_language),
                                                        palette,
                                                    )
                                                    .h(px(26.))
                                                    .kind(ButtonKind::Tab)
                                                    .icon(candidate.icon())
                                                    .selected(selected)
                                                    .border_b_2()
                                                    .border_color(if selected {
                                                        palette.accent
                                                    } else {
                                                        palette.surface.opacity(0.0)
                                                    })
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.settings_category = candidate;
                                                            cx.notify();
                                                        },
                                                    ))
                                                },
                                            )),
                                    )
                                    .child(
                                        div()
                                            .px(px(18.))
                                            .pt(px(4.))
                                            .pb(px(18.))
                                            .child(
                                                div().flex().flex_wrap().gap(px(16.)).child(
                                                    self.render_settings_category_content(
                                                        category, palette, cx,
                                                    )
                                                    .into_any_element(),
                                                ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .px(px(16.))
                                            .py(px(12.))
                                            .border_t_1()
                                            .border_color(palette.border)
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(9.))
                                                    .text_color(if issue_count == 0 {
                                                        palette.healthy
                                                    } else {
                                                        palette.danger
                                                    })
                                                    .child(if issue_count == 0 {
                                                        self.localized(
                                                            "配置有效 · 启动后冻结快照",
                                                            "Valid · Snapshot freezes on start",
                                                        )
                                                    } else {
                                                        self.localized(
                                                            "修复标记字段后才能启动",
                                                            "Resolve marked fields before starting",
                                                        )
                                                    }),
                                            )
                                            .child(
                                                GnssButton::new(
                                                    "new-analysis-start",
                                                    self.localized(
                                                        "开始处理",
                                                        "Start processing",
                                                    ),
                                                    palette,
                                                )
                                                    .kind(ButtonKind::Primary)
                                                    .icon(IconName::SquareTerminal)
                                                    .disabled(!self.can_enqueue())
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.enqueue_selected_or_draft(cx);
                                                        if this.active_run.is_some() {
                                                            this.page = AppPage::Workspace;
                                                        }
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_run_library_content(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let history_query = self.history_search.read(cx).value().to_string();
        let filter_menu_app = cx.entity();
        let current_filter = self.history_filter;
        let interface_language = self.interface_language;
        let visible_runs = self
            .history
            .iter()
            .filter(|run| {
                run.status != RunStatus::Queued
                    && Some(run.id) != self.active_run.as_ref().map(|active| active.id)
                    && history_run_matches(run, &history_query, self.history_filter)
            })
            .collect::<Vec<_>>();
        let visible_runs_empty = visible_runs.is_empty();

        div()
            .w_full()
            .child(
                div()
                    .w_full()
                    .max_w(px(metrics::PAGE_MAX_WIDTH))
                    .mx_auto()
                    .px(px(if compact {
                        metrics::PAGE_GUTTER_COMPACT
                    } else {
                        metrics::PAGE_GUTTER
                    }))
                    .py(px(28.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(24.))
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(palette.text)
                                            .child(self.localized("解算任务库", "Run library")),
                                    )
                                    .child(
                                        div()
                                            .mt(px(6.))
                                            .text_size(px(12.))
                                            .text_color(palette.muted)
                                            .child(self.localized(
                                                "查找历史解算任务、查看排队状态并重新打开结果。已完成的解算任务记录不可修改。",
                                                "Find previous runs, inspect the queue, and reopen results. Completed runs are immutable.",
                                            )),
                                    ),
                            )
                            .child(
                                GnssButton::new(
                                    "library-new-analysis",
                                    self.localized("新建解算任务", "New analysis"),
                                    palette,
                                )
                                .kind(ButtonKind::Primary)
                                .icon(IconName::Plus)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_new_task(window, cx);
                                    cx.notify();
                                })),
                            ),
                    )
                    // Toolbar: one search field for this page only, plus the
                    // explicit checked filter menu.
                    .child(
                        div()
                            .mt(px(20.))
                            .flex()
                            .when(compact, |this| this.flex_col())
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .h(px(30.))
                                    .flex_1()
                                    .min_w_0()
                                    .child(gnss_search_input(&self.history_search, palette)),
                            )
                            .child(
                                GnssButton::new(
                                    "library-history-filter",
                                    current_filter.label(interface_language),
                                    palette,
                                )
                                .kind(ButtonKind::Secondary)
                                .icon(IconName::SortDescending)
                                .selected(current_filter != HistoryFilter::All)
                                .dropdown_menu(move |menu, _, _| {
                                    menu.min_w(px(172.))
                                        .item(Self::history_filter_menu_item(
                                            filter_menu_app.clone(),
                                            HistoryFilter::All,
                                            current_filter,
                                            interface_language,
                                        ))
                                        .item(Self::history_filter_menu_item(
                                            filter_menu_app.clone(),
                                            HistoryFilter::Completed,
                                            current_filter,
                                            interface_language,
                                        ))
                                        .item(Self::history_filter_menu_item(
                                            filter_menu_app.clone(),
                                            HistoryFilter::NeedsAttention,
                                            current_filter,
                                            interface_language,
                                        ))
                                }),
                            ),
                    )
                    // Queue: flat rows under a quiet section head, never a card.
                    .children((!self.queue.is_empty()).then(|| {
                        div()
                            .mt(px(22.))
                            .border_t_1()
                            .border_color(palette.border)
                            .child(
                                div()
                                    .h(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(palette.muted)
                                            .child(self.localized("等待队列", "Queue")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_family(data_font_family())
                                            .text_color(palette.faint)
                                            .child(format!("{}", self.queue.len())),
                                    ),
                            )
                            .children(self.queue.iter().enumerate().map(|(index, run)| {
                                let id = run.id;
                                div()
                                    .h(px(metrics::LIBRARY_ROW_HEIGHT))
                                    .px(px(2.))
                                    .border_b_1()
                                    .border_color(palette.border_subtle)
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .w(px(64.))
                                            .flex_shrink_0()
                                            .text_size(px(11.))
                                            .text_color(palette.attention)
                                            .child(self.localized("排队", "Queued")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_family(data_font_family())
                                            .text_size(px(12.))
                                            .text_color(palette.text)
                                            .child(
                                                run.source_path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                                    .into_owned(),
                                            ),
                                    )
                                    .child(
                                        GnssButton::icon_only(
                                            SharedString::from(format!("library-queue-up-{id}")),
                                            IconName::ChevronUp,
                                            self.localized("上移", "Move up"),
                                            palette,
                                        )
                                        .kind(ButtonKind::Ghost)
                                        .size(ControlSize::Compact)
                                        .disabled(index == 0)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.move_queued(id, -1);
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        GnssButton::icon_only(
                                            SharedString::from(format!("library-queue-down-{id}")),
                                            IconName::ChevronDown,
                                            self.localized("下移", "Move down"),
                                            palette,
                                        )
                                        .kind(ButtonKind::Ghost)
                                        .size(ControlSize::Compact)
                                        .disabled(index + 1 == self.queue.len())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.move_queued(id, 1);
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        GnssButton::icon_only(
                                            SharedString::from(format!(
                                                "library-queue-cancel-{id}"
                                            )),
                                            IconName::CircleX,
                                            self.localized("取消排队", "Remove from queue"),
                                            palette,
                                        )
                                        .size(ControlSize::Compact)
                                        .kind(ButtonKind::QuietDestructive)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.cancel_queued(id);
                                            cx.notify();
                                        })),
                                    )
                            }))
                    }))
                    // History: a dense, flat, columnar list. The status word is
                    // bare colored text (never a filled chip) and there is no
                    // per-row icon tile or card frame (design pass).
                    .child(
                        div()
                            .mt(px(22.))
                            .h(px(28.))
                            .border_t_1()
                            .border_color(palette.border)
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(palette.muted)
                                    .child(self.localized("历史记录", "History")),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_family(data_font_family())
                                    .text_color(palette.faint)
                                    .child(match interface_language {
                                        InterfaceLanguage::SimplifiedChinese => {
                                            format!("{} 条结果", visible_runs.len())
                                        }
                                        InterfaceLanguage::English => {
                                            format!("{} results", visible_runs.len())
                                        }
                                    }),
                            ),
                    )
                    .children(visible_runs.into_iter().map(|run| {
                        let id = run.id;
                        let selected = self.selected_run_id == Some(id);
                        let (status_color, status_label) = match run.status {
                            RunStatus::Completed => {
                                (palette.healthy, self.localized("已完成", "Completed"))
                            }
                            RunStatus::Failed => (palette.danger, self.localized("失败", "Failed")),
                            RunStatus::Cancelled => {
                                (palette.unavailable, self.localized("已取消", "Cancelled"))
                            }
                            RunStatus::Interrupted => {
                                (palette.attention, self.localized("已中断", "Interrupted"))
                            }
                            _ => (palette.accent, self.localized("处理中", "Running")),
                        };
                        div()
                            .id(SharedString::from(format!("library-run-{id}")))
                            .h(px(metrics::LIBRARY_ROW_HEIGHT))
                            .border_b_1()
                            .border_color(palette.border_subtle)
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .cursor_pointer()
                            .when(
                                selected,
                                |this| {
                                    this.bg(palette.accent_wash)
                                        .border_l_2()
                                        .border_color(palette.accent)
                                        .pl(px(8.))
                                },
                            )
                            .when(!selected, |this| {
                                this.pl(px(10.)).hover(|style| style.bg(palette.hover))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.page = AppPage::Workspace;
                                this.select_run(id, window, cx);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(64.))
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .text_color(status_color)
                                    .child(status_label),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(data_font_family())
                                    .text_size(px(12.))
                                    .text_color(palette.text)
                                    .child(
                                        run.source_path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned(),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_right()
                                    .text_size(px(11.))
                                    .font_family(data_font_family())
                                    .text_color(palette.faint)
                                    .child(format!(
                                        "{} · {} 历元",
                                        run.created_at.format("%m-%d %H:%M"),
                                        run.pvt_epoch_count
                                    )),
                            )
                            .child(
                                div()
                                    .w(px(56.))
                                    .flex_shrink_0()
                                    .text_right()
                                    .text_size(px(11.))
                                    .font_family(data_font_family())
                                    .text_color(palette.muted)
                                    .child(format!("{:.0}%", run.progress * 100.0)),
                            )
                            .context_menu(self.history_context_menu(id, cx))
                    }))
                    .children(visible_runs_empty.then(|| {
                        div()
                            .mt(px(8.))
                            .h(px(160.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .text_color(palette.muted)
                            .child(IconName::Search)
                            .child(
                                div()
                                    .mt(px(10.))
                                    .text_size(px(11.))
                                    .child(self.localized(
                                        "没有符合条件的运行记录",
                                        "No runs match the current filter",
                                    )),
                            )
                    })),
            )
            .into_any_element()
    }

    fn render_component_gallery(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let language = self.interface_language;
        let state_buttons = [
            ("default", "默认", "Default", ControlState::Default),
            ("hover", "悬停", "Hover", ControlState::Hovered),
            ("pressed", "按下", "Pressed", ControlState::Pressed),
            ("focus", "焦点", "Focus", ControlState::Focused),
            ("selected", "选中", "Selected", ControlState::Selected),
            ("disabled", "禁用", "Disabled", ControlState::Disabled),
            ("busy", "忙碌", "Busy", ControlState::Busy),
            ("error", "错误", "Error", ControlState::Error),
        ]
        .into_iter()
        .map(|(id, chinese, english, state)| {
            let button = GnssButton::new(
                SharedString::from(format!("gallery-state-{id}")),
                match language {
                    InterfaceLanguage::SimplifiedChinese => chinese,
                    InterfaceLanguage::English => english,
                },
                palette,
            )
            .kind(ButtonKind::Primary)
            .on_click(cx.listener(|this, _, _, cx| {
                this.gallery_activation_count += 1;
                cx.notify();
            }));
            match state {
                ControlState::Disabled => button.disabled(true),
                ControlState::Busy => button.busy(true),
                ControlState::Selected => button.selected(true),
                _ => button.state(state),
            }
        });

        let variant_buttons = [
            ("primary", "主要", "Primary", ButtonKind::Primary),
            ("secondary", "次要", "Secondary", ButtonKind::Secondary),
            ("ghost", "安静", "Ghost", ButtonKind::Ghost),
            ("danger", "破坏性", "Destructive", ButtonKind::Destructive),
            (
                "quiet-danger",
                "安静危险",
                "Quiet danger",
                ButtonKind::QuietDestructive,
            ),
        ]
        .into_iter()
        .map(|(id, chinese, english, kind)| {
            GnssButton::new(
                SharedString::from(format!("gallery-kind-{id}")),
                match language {
                    InterfaceLanguage::SimplifiedChinese => chinese,
                    InterfaceLanguage::English => english,
                },
                palette,
            )
            .kind(kind)
            .icon(
                if matches!(kind, ButtonKind::Destructive | ButtonKind::QuietDestructive) {
                    IconName::Delete
                } else {
                    IconName::SquareTerminal
                },
            )
            .tooltip(self.localized(
                "组件状态画廊操作，可使用 Enter 或 Space 激活",
                "Component gallery action; activate with Enter or Space",
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.gallery_activation_count += 1;
                cx.notify();
            }))
        });

        let tabs = [
            (0, "总览", "Overview"),
            (1, "通道", "Channels"),
            (2, "定位", "Position"),
        ]
        .into_iter()
        .map(|(index, chinese, english)| {
            GnssButton::new(
                SharedString::from(format!("gallery-tab-{index}")),
                match language {
                    InterfaceLanguage::SimplifiedChinese => chinese,
                    InterfaceLanguage::English => english,
                },
                palette,
            )
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Compact)
            .state(if self.gallery_selected_tab == index {
                ControlState::Selected
            } else {
                ControlState::Default
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.gallery_selected_tab = index;
                this.gallery_activation_count += 1;
                cx.notify();
            }))
        });

        let button_content = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .children(state_buttons),
            )
            .child(gnss_divider(palette))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .children(variant_buttons)
                    .child(
                        GnssButton::icon_only(
                            "gallery-icon-button",
                            IconName::Settings2,
                            self.localized("打开设置", "Open settings"),
                            palette,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.gallery_activation_count += 1;
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element();

        let navigation_content = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .p(px(3.0))
                    .rounded(px(metrics::CONTROL_RADIUS))
                    .bg(palette.surface_alt)
                    .children(tabs),
            )
            .child(
                GnssRow::new(
                    "gallery-active-run",
                    self.localized("当前处理运行", "Active processing run"),
                    self.localized(
                        "进度 63% · 可继续浏览历史",
                        "63% · history remains browsable",
                    ),
                    palette,
                )
                .badge(self.localized("处理中", "Active"), BadgeKind::Accent)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssRow::new(
                    "gallery-selected-run",
                    self.localized("选中的历史运行", "Selected historical run"),
                    self.localized(
                        "只读快照 · 不影响当前处理",
                        "Read-only snapshot · active run unchanged",
                    ),
                    palette,
                )
                .state(ControlState::Selected)
                .badge(self.localized("已选择", "Selected"), BadgeKind::Healthy)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssRow::new(
                    "gallery-keyboard-row",
                    self.localized("键盘高亮行", "Keyboard-highlighted row"),
                    self.localized(
                        "Tab 到达，Enter/Space 激活",
                        "Reach with Tab, activate with Enter/Space",
                    ),
                    palette,
                )
                .state(ControlState::Focused)
                .badge(self.localized("焦点", "Focus"), BadgeKind::Warning)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssRow::new(
                    "gallery-selected-epoch",
                    self.localized("选中历元 00:02:18.400", "Selected epoch 00:02:18.400"),
                    self.localized(
                        "共享游标锁定该历元；实时处理继续推进",
                        "Shared cursor is pinned; live processing continues",
                    ),
                    palette,
                )
                .state(ControlState::Selected)
                .badge(self.localized("历元", "Epoch"), BadgeKind::Accent)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssRow::new(
                    "gallery-pinned-prn",
                    self.localized("PRN 22", "PRN 22"),
                    self.localized(
                        "固定到通道检查器，不等同于行选择",
                        "Pinned to inspector; independent from row selection",
                    ),
                    palette,
                )
                .badge(self.localized("已固定", "Pinned"), BadgeKind::Warning)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .into_any_element();

        let menu_items = div()
            .p(px(5.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                GnssMenuItem::new(
                    "gallery-menu-selected",
                    self.localized("显示全部运行", "Show all runs"),
                    palette,
                )
                .icon(IconName::LayoutDashboard)
                .checked(true)
                .state(ControlState::Selected)
                .shortcut("⌘1")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssMenuItem::new(
                    "gallery-menu-attention",
                    self.localized("仅需关注", "Needs attention"),
                    palette,
                )
                .icon(IconName::TriangleAlert)
                .shortcut("⌘2")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssMenuItem::new(
                    "gallery-menu-disabled",
                    self.localized("导出当前解", "Export current solution"),
                    palette,
                )
                .icon(IconName::File)
                .state(ControlState::Disabled),
            )
            .child(gnss_divider(palette))
            .child(
                GnssMenuItem::new(
                    "gallery-menu-delete",
                    self.localized("删除历史记录…", "Delete history entry…"),
                    palette,
                )
                .icon(IconName::Delete)
                .destructive(true)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssMenuItem::new(
                    "gallery-menu-extra-1",
                    self.localized("定位到源文件", "Reveal source file"),
                    palette,
                )
                .icon(IconName::FolderOpen)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            )
            .child(
                GnssMenuItem::new(
                    "gallery-menu-extra-2",
                    self.localized("复制运行标识", "Copy run identifier"),
                    palette,
                )
                .icon(IconName::Copy)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gallery_activation_count += 1;
                    cx.notify();
                })),
            );

        let menu_content = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(gnss_scroll_area(
                "gallery-menu-scroll",
                150.0,
                menu_items,
                palette,
            ))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(palette.muted)
                    .child(self.localized(
                        "滚动条沿用 gpui-component 行为，并由组件主题令牌着色。",
                        "Scrollbar behavior stays delegated and is colored by component tokens.",
                    )),
            )
            .into_any_element();

        let form_content = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(gnss_input(
                &self.gallery_input,
                palette,
                ControlState::Default,
            ))
            .child(
                div()
                    .child(gnss_input(
                        &self.gallery_input,
                        palette,
                        ControlState::Error,
                    ))
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(10.0))
                            .text_color(palette.danger)
                            .child(self.localized(
                                "错误状态同时使用边框、文本与语义色",
                                "Error state combines border, text, and semantic color",
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(16.0))
                    .child(
                        GnssSwitch::new(
                            "gallery-switch",
                            self.localized("实时跟随", "Live follow"),
                            self.gallery_switch_checked,
                            palette,
                        )
                        .on_toggle(cx.listener(|this, checked, _, cx| {
                            this.gallery_switch_checked = *checked;
                            this.gallery_activation_count += 1;
                            cx.notify();
                        })),
                    )
                    .child(
                        GnssSwitch::new(
                            "gallery-switch-disabled",
                            self.localized("禁用开关", "Disabled switch"),
                            false,
                            palette,
                        )
                        .state(ControlState::Disabled),
                    ),
            )
            .child(gnss_progress(63.0, palette, false))
            .child(gnss_progress(42.0, palette, true))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(gnss_badge(
                        self.localized("已锁定", "Locked"),
                        BadgeKind::Healthy,
                        palette,
                    ))
                    .child(gnss_badge(
                        self.localized("重捕获", "Reacquiring"),
                        BadgeKind::Warning,
                        palette,
                    ))
                    .child(gnss_badge(
                        self.localized("不可用", "Unavailable"),
                        BadgeKind::Neutral,
                        palette,
                    ))
                    .child(gnss_badge(
                        self.localized("失败", "Failed"),
                        BadgeKind::Danger,
                        palette,
                    )),
            )
            .into_any_element();

        div()
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .font_family(interface_font_family())
            .flex()
            .flex_col()
            .child(self.render_title_bar(palette, false, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(1120.0))
                            .mx_auto()
                            .px(px(28.0))
                            .py(px(24.0))
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .justify_between()
                                    .child(
                                        div()
                                            .child(
                                                div()
                                                    .text_size(px(20.0))
                                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                                    .child(self.localized(
                                                        "Phase 2 · 组件状态画廊",
                                                        "Phase 2 · Component state gallery",
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(5.0))
                                                    .text_size(px(11.0))
                                                    .text_color(palette.muted)
                                                    .child(self.localized(
                                                        "Tab 导航；Enter/Space 与鼠标激活遵循同一回调。",
                                                        "Tab navigates; Enter/Space and mouse share one activation path.",
                                                    )),
                                            ),
                                    )
                                    .child(gnss_badge(
                                        format!(
                                            "{} {}",
                                            self.localized("激活", "Activations"),
                                            self.gallery_activation_count
                                        ),
                                        BadgeKind::Accent,
                                        palette,
                                    )),
                            )
                            .child(
                                div()
                                    .mt(px(20.0))
                                    .grid()
                                    .grid_cols(2)
                                    .gap(px(14.0))
                                    .child(self.render_gallery_card(
                                        self.localized("按钮与状态", "Buttons and states"),
                                        self.localized(
                                            "八种优先级状态、四种语义变体和具名图标按钮。",
                                            "Eight priority states, four semantic variants, and a named icon button.",
                                        ),
                                        button_content,
                                        palette,
                                    ))
                                    .child(self.render_gallery_card(
                                        self.localized("标签、运行行与菜单行", "Tabs, run rows, and menu rows"),
                                        self.localized(
                                            "当前运行、选中运行与键盘高亮保持不同视觉身份。",
                                            "Active, selected, and keyboard-highlighted rows remain distinct.",
                                        ),
                                        navigation_content,
                                        palette,
                                    ))
                                    .child(self.render_gallery_card(
                                        self.localized("输入、开关与进度", "Inputs, switches, and progress"),
                                        self.localized(
                                            "行为复杂的文本编辑继续复用 gpui-component，外层由 GNSScope 定义。",
                                            "Behavior-heavy text editing stays delegated; GNSScope owns its frame.",
                                        ),
                                        form_content,
                                        palette,
                                    ))
                                    .child(self.render_gallery_card(
                                        self.localized("菜单与滚动", "Menus and scrolling"),
                                        self.localized(
                                            "选中、禁用、破坏性菜单项和令牌化滚动条。",
                                            "Selected, disabled, destructive menu items and a tokenized scrollbar.",
                                        ),
                                        menu_content,
                                        palette,
                                    )),
                            ),
                    ),
            )
            .child(self.render_status_bar(
                palette,
                self.runs_sidebar_visible,
                false,
                false,
                cx,
            ))
            .into_any_element()
    }

    fn render_gallery_card(
        &self,
        title: &'static str,
        description: &'static str,
        content: AnyElement,
        palette: Palette,
    ) -> impl IntoElement {
        div()
            .min_h(px(230.0))
            .p(px(16.0))
            .rounded(px(6.0))
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .mt(px(4.0))
                    .mb(px(14.0))
                    .text_size(px(10.0))
                    .text_color(palette.muted)
                    .child(description),
            )
            .child(content)
    }

    /// One interface preference: label and explanation on the left, the
    /// control on the right, separated by a hairline. No icon tile and no
    /// card frame — a settings document reads as a list, not a dashboard
    /// (design pass; spec §7 forbids card grids and decorative chrome).
    fn render_interface_setting_section(
        &self,
        title: &'static str,
        description: &'static str,
        control: AnyElement,
        palette: Palette,
    ) -> impl IntoElement {
        div()
            .min_h(px(metrics::FORM_ROW_HEIGHT + 18.0))
            .py(px(14.))
            .border_b_1()
            .border_color(palette.border_subtle)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(32.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(palette.text)
                            .child(title),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(11.))
                            .text_color(palette.muted)
                            .child(description),
                    ),
            )
            .child(div().flex_shrink_0().child(control))
    }

    fn render_interface_settings_content(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let language = self.interface_language;
        let theme_control = div()
            .flex()
            .flex_wrap()
            .gap(px(2.))
            .children(
                [
                    (ThemePreference::System, IconName::Palette, "系统", "System"),
                    (ThemePreference::Light, IconName::Sun, "浅色", "Light"),
                    (ThemePreference::Dark, IconName::Moon, "深色", "Dark"),
                ]
                .into_iter()
                .map(|(preference, icon, chinese, english)| {
                    let active = self.theme_preference == preference;
                    GnssButton::new(
                        SharedString::from(format!("interface-theme-{}", preference.value())),
                        match language {
                            InterfaceLanguage::SimplifiedChinese => chinese,
                            InterfaceLanguage::English => english,
                        },
                        palette,
                    )
                    .icon(icon)
                    .kind(ButtonKind::Toggle)
                    .selected(active)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_theme_preference(preference, window, cx);
                        cx.notify();
                    }))
                }),
            )
            .into_any_element();

        let language_control = div()
            .flex()
            .flex_wrap()
            .gap(px(2.))
            .children(
                [
                    (InterfaceLanguage::SimplifiedChinese, "简体中文"),
                    (InterfaceLanguage::English, "English"),
                ]
                .into_iter()
                .map(|(option, label)| {
                    let active = language == option;
                    GnssButton::new(
                        SharedString::from(format!("interface-language-{}", option.value())),
                        label,
                        palette,
                    )
                    .icon(IconName::Globe)
                    .kind(ButtonKind::Toggle)
                    .selected(active)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_interface_language(option, window, cx);
                        cx.notify();
                    }))
                }),
            )
            .into_any_element();

        let timeline_control = div()
            .flex()
            .items_center()
            .justify_end()
            .child(
                GnssSwitch::new(
                    "interface-timeline-visible",
                    self.localized("显示时间轴", "Show timeline"),
                    self.timeline_visible,
                    palette,
                )
                .on_toggle(cx.listener(|this, checked, _, cx| {
                    this.set_timeline_visibility(*checked);
                    cx.notify();
                })),
            )
            .into_any_element();

        div()
            .w_full()
            .child(
                div()
                    .w_full()
                    .max_w(px(metrics::PAGE_MAX_WIDTH))
                    .mx_auto()
                    .px(px(metrics::PAGE_GUTTER))
                    .py(px(28.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(24.))
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(palette.text)
                                            .child(self.localized(
                                                "界面设置",
                                                "Interface settings",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .mt(px(6.))
                                            .text_size(px(12.))
                                            .text_color(palette.muted)
                                            .child(self.localized(
                                                "统一管理 GNSScope 的外观与工作区显示偏好。接收机配置属于解算任务，不在此处修改。",
                                                "Manage GNSScope appearance and workspace preferences. Receiver configuration belongs to a run and is not edited here.",
                                            )),
                                    ),
                            )
                            .child(
                                GnssButton::icon_only(
                                    "close-interface-settings",
                                    IconName::ArrowLeft,
                                    self.localized("返回工作台", "Back to workspace"),
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.page = AppPage::Workspace;
                                    cx.notify();
                                })),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(24.))
                            .border_t_1()
                            .border_color(palette.border)
                            .child(self.render_interface_setting_section(
                                self.localized("外观主题", "Appearance theme"),
                                self.localized(
                                    "跟随系统，或固定使用浅色与深色主题。更改会立即应用。",
                                    "Follow the system or pin a light or dark theme. Changes apply immediately.",
                                ),
                                theme_control,
                                palette,
                            ))
                            .child(self.render_interface_setting_section(
                                self.localized("界面语言", "Interface language"),
                                self.localized(
                                    "选择界面导航与设置页面使用的语言。",
                                    "Choose the language used by interface navigation and settings.",
                                ),
                                language_control,
                                palette,
                            ))
                            .child(self.render_interface_setting_section(
                                self.localized("Epoch 时间轴", "Epoch timeline"),
                                self.localized(
                                    "控制工作台底部的 epoch 时间线仪器是否可见。",
                                    "Control whether the epoch timeline instrument is visible beneath the workbench.",
                                ),
                                timeline_control.into_any_element(),
                                palette,
                            )),
                    ),
            )
            .into_any_element()
    }

    fn setting_issue_message(&self, field: &'static str) -> Option<&'static str> {
        self.setting_parse_errors.get(field).copied().or_else(|| {
            crate::state::validate_settings(&self.settings)
                .into_iter()
                .find(|issue| issue.field == field)
                .map(|issue| issue.message)
        })
    }

    fn render_setting_input(
        &self,
        label: &'static str,
        field: &'static str,
        unit: &'static str,
        palette: Palette,
    ) -> impl IntoElement {
        let input = self.setting_inputs.get(field).expect("setting input");
        let issue = self.setting_issue_message(field);
        div()
            .w(px(metrics::FORM_FIELD_WIDTH))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(if issue.is_some() {
                        palette.danger
                    } else {
                        palette.muted
                    })
                    .child(label)
                    .child(div().text_color(palette.faint).child(unit)),
            )
            .child(div().mt(px(5.)).h(px(30.)).child(gnss_input(
                input,
                palette,
                if issue.is_some() {
                    ControlState::Error
                } else {
                    ControlState::Default
                },
            )))
            .children(issue.map(|message| {
                div()
                    .mt(px(3.))
                    .text_size(px(9.))
                    .text_color(palette.danger)
                    .child(message)
            }))
    }

    fn render_config_drawer(
        &self,
        palette: Palette,
        compact: bool,
        full_width: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let issues = self.settings_issues();
        let category = self.settings_category;
        let issue_count = issues.len() + self.setting_parse_errors.len();
        let icon_only_categories = compact && !full_width;
        div()
            .when(full_width, |this| this.w_full().flex_1())
            .when(!full_width, |this| {
                this.w(if compact { px(372.) } else { px(460.) })
                    .flex_shrink_0()
            })
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.surface)
            .border_l_1()
            .border_color(palette.border)
            .child(
                div()
                    .h(px(54.))
                    .flex_shrink_0()
                    .px(px(14.))
                    .border_b_1()
                    .border_color(palette.border)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(
                                div()
                                    .size(px(26.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(3.))
                                    .bg(palette.accent.opacity(0.11))
                                    .text_color(palette.accent)
                                    .child(IconName::Inspector),
                            )
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(palette.text)
                                            .child(self.localized(
                                                "接收机检查器",
                                                "Receiver inspector",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.))
                                            .text_color(palette.muted)
                                            .child(self.localized(
                                                "下一次解算的配置草稿",
                                                "Configuration draft for the next run",
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(
                                GnssButton::icon_only(
                                    "reset-receiver-settings",
                                    IconName::Undo2,
                                    self.localized(
                                        "恢复接收机默认参数，但保留当前 IQ 文件",
                                        "Restore receiver defaults while keeping the current IQ file",
                                    ),
                                    palette,
                                )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.reset_receiver_settings(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                GnssButton::icon_only(
                                    "close-settings-drawer",
                                    IconName::CircleX,
                                    self.localized(
                                        "关闭接收机配置检查器",
                                        "Close receiver inspector",
                                    ),
                                    palette,
                                )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.side_drawer = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .children((issue_count > 0).then(|| {
                div()
                    .min_h(px(34.))
                    .flex_shrink_0()
                    .px(px(14.))
                    .py(px(7.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(palette.danger.opacity(0.09))
                    .border_b_1()
                    .border_color(palette.danger.opacity(0.35))
                    .text_size(px(9.))
                    .text_color(palette.danger)
                    .child(IconName::TriangleAlert)
                    .child(match self.interface_language {
                        InterfaceLanguage::SimplifiedChinese => format!(
                            "{issue_count} 项配置需要处理{}",
                            issues
                                .first()
                                .map(|issue| format!(" · {}", issue.message))
                                .unwrap_or_default()
                        ),
                        InterfaceLanguage::English => {
                            format!("{issue_count} configuration issues")
                        }
                    })
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .w(if icon_only_categories {
                                px(54.)
                            } else {
                                px(118.)
                            })
                            .flex_shrink_0()
                            .py(px(8.))
                            .px(px(6.))
                            .bg(palette.sidebar)
                            .border_r_1()
                            .border_color(palette.border)
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children(SettingsCategory::ALL.into_iter().map(|candidate| {
                                let selected = category == candidate;
                                let id = SharedString::from(format!(
                                    "settings-category-{:?}", candidate
                                ));
                                if icon_only_categories {
                                    GnssButton::icon_only(
                                        id,
                                        candidate.icon(),
                                        candidate.description(self.interface_language),
                                        palette,
                                    )
                                    .w_full()
                                    .selected(selected)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.settings_category = candidate;
                                        cx.notify();
                                    }))
                                    .into_any_element()
                                } else {
                                    GnssButton::new(
                                        id,
                                        candidate.label(self.interface_language),
                                        palette,
                                    )
                                    .w_full()
                                    .kind(ButtonKind::Ghost)
                                    .icon(candidate.icon())
                                    .tooltip(candidate.description(self.interface_language))
                                    .selected(selected)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.settings_category = candidate;
                                        cx.notify();
                                    }))
                                    .into_any_element()
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(
                                div()
                                    .px(px(16.))
                                    .pt(px(14.))
                                    .pb(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(palette.text)
                                            .child(category.label(self.interface_language)),
                                    )
                                    .child(
                                        div()
                                            .mt(px(2.))
                                            .text_size(px(9.))
                                            .text_color(palette.muted)
                                            .child(category.description(self.interface_language)),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(16.))
                                    .pb(px(20.))
                                    .child(
                                        self.render_settings_category_content(
                                            category, palette, cx,
                                        )
                                        .into_any_element(),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .min_h(px(32.))
                    .flex_shrink_0()
                    .px(px(14.))
                    .py(px(7.))
                    .border_t_1()
                    .border_color(palette.border)
                    .text_size(px(9.))
                    .text_color(if issue_count == 0 {
                        palette.healthy
                    } else {
                        palette.danger
                    })
                    .child(if issue_count == 0 {
                        self.localized(
                            "配置有效 · 入队后快照冻结",
                            "Configuration valid · Snapshot freezes when queued",
                        )
                    } else {
                        self.localized(
                            "修复配置问题后才能开始解算",
                            "Resolve configuration issues before processing",
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_settings_category_content(
        &self,
        category: SettingsCategory,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match category {
            SettingsCategory::Input => self.render_input_settings(palette, cx).into_any_element(),
            SettingsCategory::Acquisition => {
                self.render_acquisition_settings(palette).into_any_element()
            }
            SettingsCategory::Tracking => self
                .render_tracking_settings(palette, cx)
                .into_any_element(),
            SettingsCategory::Pvt => self.render_pvt_settings(palette, cx).into_any_element(),
            SettingsCategory::Performance => self
                .render_performance_settings(palette, cx)
                .into_any_element(),
        }
    }

    fn render_input_settings(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let iq_format_app = cx.entity();
        let current_iq_format = self.settings.iq_format;
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(16.))
            .child(Self::config_group_title(
                self.localized("采样", "Sampling"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("采样率", "Sample rate"),
                "sample_rate_hz",
                "Msps",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("中频", "Intermediate frequency"),
                "intermediate_frequency_hz",
                "MHz",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("读取 chunk", "Read chunk"),
                "chunk_ms",
                "ms",
                palette,
            ))
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .child(self.localized("IQ 格式", "IQ format")),
                    )
                    .child(
                        GnssButton::new(
                            "drawer-iq-format",
                            format!("{:?}", self.settings.iq_format),
                            palette,
                        )
                        .kind(ButtonKind::Secondary)
                        .icon(IconName::Replace)
                        .dropdown_menu(move |menu, _, _| {
                            menu.min_w(px(172.))
                                .item(Self::iq_format_menu_item(
                                    iq_format_app.clone(),
                                    crate::GuiIqFormat::Int8Interleaved,
                                    current_iq_format,
                                ))
                                .item(Self::iq_format_menu_item(
                                    iq_format_app.clone(),
                                    crate::GuiIqFormat::Int16Interleaved,
                                    current_iq_format,
                                ))
                        }),
                    ),
            )
            .child(Self::config_group_title(
                self.localized("导航来源", "Navigation source"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("RINEX 导航文件", "RINEX navigation file"),
                "rinex_nav_path",
                self.localized("可选路径", "optional path"),
                palette,
            ))
            .child(self.render_setting_toggle(
                self.localized(
                    "允许环境变量导航文件",
                    "Allow navigation file from environment",
                ),
                self.settings.use_rinex_nav_environment,
                "use_rinex_nav_environment",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                self.localized(
                    "自动发现同目录导航文件",
                    "Discover colocated navigation file",
                ),
                self.settings.auto_discover_colocated_nav,
                "auto_discover_colocated_nav",
                palette,
                cx,
            ))
            .child(self.render_setting_input(
                self.localized("初始位置", "Initial position"),
                "initial_position_lla_deg_m",
                self.localized("纬度, 经度, 高程", "lat, lon, altitude"),
                palette,
            ))
    }

    fn render_acquisition_settings(&self, palette: Palette) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(16.))
            .child(Self::config_group_title(
                self.localized("搜索窗口", "Search window"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("捕获窗口", "Acquisition window"),
                "acquisition_window_ms",
                "ms",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("非相干累加", "Non-coherent integrations"),
                "acquisition_noncoherent_integrations",
                self.localized("次", "count"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("捕获阈值", "Acquisition threshold"),
                "acquisition_threshold",
                "",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("最大多普勒", "Maximum Doppler"),
                "acquisition_doppler_max_hz",
                "Hz",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("多普勒步进", "Doppler step"),
                "acquisition_doppler_step_hz",
                "Hz",
                palette,
            ))
            .child(Self::config_group_title(
                self.localized("调度策略", "Scheduling policy"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("冷启动扫描周期", "Cold acquisition interval"),
                "cold_acquisition_interval_ms",
                "ms",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("重捕获周期", "Reacquisition interval"),
                "reacquisition_interval_ms",
                "ms",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("单次最大 PRN", "Maximum PRNs per scan"),
                "max_acquisition_prns_per_scan",
                "1-32",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("固定 PRN", "Fixed PRNs"),
                "fixed_prns",
                self.localized("逗号分隔", "comma-separated"),
                palette,
            ))
    }

    fn render_tracking_settings(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(16.))
            .child(Self::config_group_title(
                self.localized("环路参数", "Loop parameters"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("DLL 带宽", "DLL bandwidth"),
                "dll_bandwidth_hz",
                "Hz",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("PLL 带宽", "PLL bandwidth"),
                "pll_bandwidth_hz",
                "Hz",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("早迟间距", "Early-late spacing"),
                "early_late_spacing_chips",
                "chips",
                palette,
            ))
            .child(self.render_setting_toggle(
                self.localized("载波辅助码环", "Carrier-aided code loop"),
                self.settings.carrier_aiding,
                "carrier_aiding",
                palette,
                cx,
            ))
            .child(Self::config_group_title(
                self.localized("锁定与准入", "Lock and admission"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("PLL lock 阈值", "PLL lock threshold"),
                "minimum_pll_lock_metric",
                "0-1",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("最低 C/N0", "Minimum C/N0"),
                "minimum_cn0_db_hz",
                "dB-Hz",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("准入观察期", "Admission probation"),
                "probation_good_epochs",
                "epochs",
                palette,
            ))
            .child(Self::config_group_title(
                self.localized("C/N0 估计", "C/N0 estimation"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("C/N0 窗长", "C/N0 window"),
                "cn0_samples",
                "samples",
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("C/N0 平滑系数", "C/N0 smoothing alpha"),
                "cn0_smoother_alpha",
                "0-1",
                palette,
            ))
    }

    fn render_pvt_settings(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(16.))
            .child(Self::config_group_title(
                self.localized("输出节奏", "Output cadence"),
                palette,
            ))
            .child(self.render_setting_input(
                self.localized("PVT 输出 epoch", "PVT output epoch"),
                "pvt_output_rate_ms",
                "ms",
                palette,
            ))
            .child(self.render_setting_toggle(
                self.localized("epoch 多普勒平均", "Average Doppler over epoch"),
                self.settings.average_doppler_over_pvt_epoch,
                "average_doppler_over_pvt_epoch",
                palette,
                cx,
            ))
            .child(Self::config_group_title(
                self.localized("滤波与完整性", "Filtering and integrity"),
                palette,
            ))
            .child(self.render_setting_toggle(
                self.localized("载波平滑", "Carrier smoothing"),
                self.settings.enable_carrier_smoothing,
                "enable_carrier_smoothing",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                "Kalman",
                self.settings.enable_kalman,
                "enable_kalman",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                self.localized(
                    "整数毫秒模糊度解算",
                    "Integer-millisecond ambiguity resolution",
                ),
                self.settings.enable_integer_ms_ambiguity_resolution,
                "enable_integer_ms_ambiguity_resolution",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                self.localized("RAIM 故障检测与排除", "RAIM fault detection and exclusion"),
                self.settings.enable_raim,
                "enable_raim",
                palette,
                cx,
            ))
    }

    fn render_performance_settings(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(16.))
            .child(Self::config_group_title(
                self.localized("并行处理", "Parallel processing"),
                palette,
            ))
            .child(self.render_setting_toggle(
                self.localized("并行捕获", "Parallel acquisition"),
                self.settings.parallel_acquisition,
                "parallel_acquisition",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                self.localized("并行跟踪", "Parallel tracking"),
                self.settings.parallel_tracking,
                "parallel_tracking",
                palette,
                cx,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .p(px(11.))
                    .rounded(px(5.))
                    .bg(palette.surface_alt)
                    .text_size(px(10.))
                    .text_color(palette.muted)
                    .child(self.localized(
                        "并行处理只影响计算调度，不改变确定性的 PRN 合并顺序。",
                        "Parallelism changes compute scheduling only; deterministic PRN merge order is preserved.",
                    )),
            )
    }

    fn config_group_title(title: &'static str, palette: Palette) -> impl IntoElement {
        div()
            .w_full()
            .mt(px(10.))
            .pt(px(10.))
            .border_t_1()
            .border_color(palette.border_subtle)
            .text_size(px(11.))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(palette.faint)
            .child(title)
    }

    fn render_setting_toggle(
        &self,
        label: &'static str,
        enabled: bool,
        field: &'static str,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .min_h(px(38.))
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(palette.border.opacity(0.45))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(palette.muted)
                    .child(label),
            )
            .child(
                GnssSwitch::new(
                    SharedString::from(format!("{field}-toggle")),
                    if enabled {
                        self.localized("已开启", "Enabled")
                    } else {
                        self.localized("已关闭", "Disabled")
                    },
                    enabled,
                    palette,
                )
                .on_toggle(cx.listener(move |this, _, _, cx| {
                    this.toggle_setting(field);
                    cx.notify();
                })),
            )
    }

    #[allow(dead_code)]
    fn render_legacy_workspace_tabs(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .h(px(40.))
            .flex_shrink_0()
            .px(px(12.))
            .bg(palette.surface)
            .border_b_1()
            .border_color(palette.border)
            .flex()
            .items_center()
            .gap(px(2.))
            .children(WorkspaceView::ALL.into_iter().map(|view| {
                let selected = self.workspace_view == view;
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(if selected {
                        palette.accent
                    } else {
                        palette.surface
                    })
                    .child(
                        Button::new(SharedString::from(format!("workspace-view-{}", view.id())))
                            .ghost()
                            .rounded(px(0.))
                            .icon(view.icon())
                            .label(view.label(self.interface_language))
                            .text_color(if selected {
                                palette.text
                            } else {
                                palette.muted
                            })
                            .bg(if selected {
                                palette.surface_alt
                            } else {
                                palette.surface
                            })
                            .tooltip(view.description(self.interface_language))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.workspace_view = view;
                                this.focused_plot = None;
                                cx.notify();
                            })),
                    )
            }))
    }

    fn render_workspace_tabs(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected_run = self.selected_run();
        let selected_is_active = self.selected_run_id.is_some()
            && self.selected_run_id == self.active_run.as_ref().map(|run| run.id);
        let status_color = selected_run
            .map(|run| match run.status {
                RunStatus::Completed => palette.healthy,
                RunStatus::Failed => palette.danger,
                RunStatus::Queued | RunStatus::Cancelled | RunStatus::Interrupted => {
                    palette.warning
                }
                _ => palette.accent,
            })
            .unwrap_or(palette.muted);
        let status_label = selected_run
            .map(|run| match run.status {
                RunStatus::Completed => self.localized("已完成", "Completed"),
                RunStatus::Failed => self.localized("失败", "Failed"),
                RunStatus::Queued => self.localized("已排队", "Queued"),
                RunStatus::Cancelled => self.localized("已取消", "Cancelled"),
                RunStatus::Interrupted => self.localized("已中断", "Interrupted"),
                _ => self.localized("处理中", "Running"),
            })
            .unwrap_or(self.localized("未选择", "No run"));
        let status_text = status_label.to_string();
        // Inline processing percentage (data 13px accent); only shown while
        // the run is actually processing (spec §3.3).
        let status_percent = selected_run
            .filter(|run| matches!(run.status, RunStatus::Processing))
            .map(|run| processing_percent_label(run.progress));
        // Metadata segments: localized label + real value pairs (spec §3.3);
        // missing segments are omitted, never zero-filled (spec §7.7).
        let run_metadata = {
            let mut segments: Vec<crate::ui::workbench::MetadataSegment> = Vec::new();
            let (processed, total, rate_hz, if_hz) = selected_run.map_or(
                (
                    0,
                    0,
                    self.settings.sample_rate_hz,
                    self.settings.intermediate_frequency_hz,
                ),
                |run| {
                    (
                        run.processed_samples,
                        run.total_samples,
                        run.settings.sample_rate_hz,
                        run.settings.intermediate_frequency_hz,
                    )
                },
            );
            if let Some(run) = selected_run {
                segments.push(crate::ui::workbench::MetadataSegment::new(
                    self.localized("开始", "Started"),
                    run.created_at.format("%H:%M:%S").to_string(),
                ));
            }
            // Elapsed: active runs use the processed count, completed runs
            // the full count; only emitted for a real, usable value.
            let elapsed_samples = if selected_is_active {
                processed
            } else if selected_run.is_some() {
                total
            } else {
                0
            };
            if let Some(elapsed) = crate::ui::workbench::format_elapsed(
                self.localized("已用时", "Elapsed"),
                elapsed_samples,
                rate_hz,
            ) {
                segments.push(elapsed);
            }
            if total > 0 {
                segments.push(crate::ui::workbench::format_sample_count(
                    self.localized("采样数", "Samples"),
                    total,
                ));
            }
            if let Some(if_segment) = crate::ui::workbench::format_intermediate_frequency(
                self.localized("中频", "IF"),
                if_hz,
            ) {
                segments.push(if_segment);
            }
            segments
        };

        div()
            .h(px(metrics::WORKSPACE_HEADER_HEIGHT))
            .flex_shrink_0()
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .rounded(px(crate::visual_system::radius_panel()))
            .overflow_hidden()
            .child(
                div()
                    .h(px(metrics::WORKSPACE_CONTEXT_HEIGHT))
                    .px(px(14.))
                    .bg(palette.surface_raised)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    // One identity line: title, status, then the run's frozen
                    // metadata set as inline key/value pairs behind a rule.
                    // Collapsing the old two-row stack into this line is what
                    // gives the panel a calm, single-baseline header.
                    .child(
                        div()
                            .min_w_0()
                            .when(!compact, |this| this.max_w(px(720.)))
                            .when(compact, |this| {
                                this.h(px(20.)).overflow_hidden().text_size(px(14.))
                            })
                            .when(!compact, |this| {
                                this.text_size(px(20.))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            })
                            .font_family(data_font_family())
                            .text_color(palette.text)
                            .truncate()
                            .child(self.file_label()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().size(px(6.)).rounded_full().bg(status_color))
                            .children((!compact).then(|| {
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(6.))
                                    .text_size(px(12.))
                                    .font_family(data_font_family())
                                    .text_color(status_color)
                                    .child(status_text)
                                    .children(status_percent.map(|percent| {
                                        div()
                                            .text_size(px(12.))
                                            .font_family(data_font_family())
                                            .text_color(palette.accent)
                                            .child(percent)
                                    }))
                            })),
                    )
                    .children((!compact && !run_metadata.is_empty()).then(|| {
                        div()
                            .flex_shrink_0()
                            .w(px(1.))
                            .h(px(14.))
                            .bg(palette.border)
                    }))
                    .children((!compact).then(|| {
                        div()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .gap(px(4.))
                            .overflow_hidden()
                            .text_size(px(11.))
                            .text_color(palette.muted)
                            .children(run_metadata.iter().enumerate().flat_map(
                                |(index, segment)| {
                                    let separator = (index > 0).then(|| {
                                        div()
                                            .flex_shrink_0()
                                            .font_family(data_font_family())
                                            .text_color(palette.faint)
                                            .child(crate::ui::workbench::SEGMENT_SEPARATOR)
                                    });
                                    separator.into_iter().chain([
                                        div()
                                            .flex_shrink_0()
                                            .font_family(interface_font_family())
                                            .whitespace_nowrap()
                                            .child(segment.label),
                                        div()
                                            .flex_shrink_0()
                                            .font_family(data_font_family())
                                            .whitespace_nowrap()
                                            .child(segment.value.clone()),
                                    ])
                                },
                            ))
                    }))
                    .child(div().flex_1())
                    .children((self.active_run.is_some() && !self.follow_live).then(|| {
                        GnssButton::new(
                            "workspace-return-live",
                            self.localized("返回实时", "Return live"),
                            palette,
                        )
                        .kind(ButtonKind::Ghost)
                        .icon(IconName::Redo2)
                        .tooltip(self.localized(
                            "切回正在处理的运行和最新 epoch",
                            "Return to the active run and newest epoch",
                        ))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.return_to_live(window, cx);
                            cx.notify();
                        }))
                    }))
                    .child(if compact {
                        GnssButton::icon_only(
                            "workspace-event-log",
                            IconName::Inbox,
                            self.localized("事件日志", "Event log"),
                            palette,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.workspace_view = WorkspaceView::Navigation;
                            this.focused_plot = None;
                            cx.notify();
                        }))
                        .into_any_element()
                    } else {
                        GnssButton::new(
                            "workspace-event-log",
                            self.localized("事件日志", "Event log"),
                            palette,
                        )
                        .icon(IconName::Inbox)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.workspace_view = WorkspaceView::Navigation;
                            this.focused_plot = None;
                            cx.notify();
                        }))
                        .into_any_element()
                    })
                    .child(if compact {
                        GnssButton::icon_only(
                            "workspace-run-configuration",
                            IconName::Settings2,
                            self.localized("运行配置", "Run configuration"),
                            palette,
                        )
                        .selected(self.run_configuration_visible)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_run_configuration_visibility(!this.run_configuration_visible);
                            cx.notify();
                        }))
                        .into_any_element()
                    } else {
                        GnssButton::new(
                            "workspace-run-configuration",
                            self.localized("运行配置", "Run configuration"),
                            palette,
                        )
                        .icon(IconName::Settings2)
                        .selected(self.run_configuration_visible)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_run_configuration_visibility(!this.run_configuration_visible);
                            cx.notify();
                        }))
                        .into_any_element()
                    })
                    .children(selected_is_active.then(|| {
                        GnssButton::icon_only(
                            "workspace-cancel-active",
                            IconName::CircleX,
                            self.localized("取消当前处理", "Cancel active run"),
                            palette,
                        )
                        .kind(ButtonKind::QuietDestructive)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_active();
                            cx.notify();
                        }))
                    }))
                    .children((selected_run.is_some() && !selected_is_active).then(|| {
                        GnssButton::icon_only(
                            "workspace-reprocess",
                            IconName::Replace,
                            self.localized(
                                "使用此运行的配置重新处理",
                                "Reprocess with this run configuration",
                            ),
                            palette,
                        )
                        .disabled(!self.can_enqueue())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.enqueue_selected_or_draft(cx);
                            cx.notify();
                        }))
                    })),
            )
            .child(
                div()
                    .h(px(metrics::WORKSPACE_TABS_HEIGHT))
                    .px(px(14.))
                    .pb(px(3.))
                    .flex()
                    .items_end()
                    .gap(px(2.))
                    .children(WorkspaceView::ALL.into_iter().map(|view| {
                        let selected = self.workspace_view == view;
                        GnssButton::new(
                            SharedString::from(format!("workbench-view-{}", view.id())),
                            view.label(self.interface_language),
                            palette,
                        )
                        .h_full()
                        .kind(ButtonKind::Tab)
                        .rounded(px(0.))
                        .selected(selected)
                        .border_b_2()
                        .border_color(if selected {
                            palette.accent
                        } else {
                            palette.surface.opacity(0.0)
                        })
                        .tooltip(view.description(self.interface_language))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.workspace_view = view;
                            this.focused_plot = None;
                            cx.notify();
                        }))
                    })),
            )
    }

    fn render_workspace_empty(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .child(
                div()
                    .w_full()
                    .max_w(px(560.))
                    .p(px(28.))
                    .rounded(px(8.))
                    .bg(palette.surface)
                    .border_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .size(px(42.))
                            .rounded(px(7.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(palette.accent.opacity(0.10))
                            .text_color(palette.accent)
                            .child(IconName::LayoutDashboard),
                    )
                    .child(
                        div()
                            .mt(px(18.))
                            .text_size(px(18.))
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .text_color(palette.text)
                            .child(self.localized(
                                "从一次分析开始",
                                "Start with an analysis",
                            )),
                    )
                    .child(
                        div()
                            .mt(px(7.))
                            .text_size(px(11.))
                            .text_color(palette.muted)
                            .child(self.localized(
                                "工作台用于监控正在运行的接收机，并复盘已持久化的 PVT、通道和导航事件。",
                                "The workspace monitors the active receiver and replays persisted PVT, channel, and navigation results.",
                            )),
                    )
                    .child(
                        div()
                            .mt(px(18.))
                            .flex()
                            .gap(px(8.))
                            .child(
                                GnssButton::new(
                                    "empty-new-analysis",
                                    self.localized("新建分析", "New analysis"),
                                    palette,
                                )
                                    .kind(ButtonKind::Primary)
                                    .icon(IconName::Plus)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_new_task(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                GnssButton::new(
                                    "empty-open-library",
                                    self.localized("打开运行库", "Open run library"),
                                    palette,
                                )
                                    .icon(IconName::ChartPie)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.page = AppPage::RunLibrary;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_pvt_summary(
        &self,
        palette: Palette,
        _compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pvt = self.current_pvt();
        // The frozen 15-row table contract (five columns, full-precision
        // values, honest "—" cells, quiet 有效/不可用/无效 status) lives in
        // `plots::pvt` (design-refresh spec §3.8); this slice only supplies
        // the selected-epoch data and the pane chrome.
        let table = crate::plots::pvt::summary_table(
            crate::plots::pvt::SummaryInput {
                pvt,
                position_displayable: pvt.is_some_and(pvt_position_is_displayable),
                preview: crate::plots::pvt::preview_mode(),
            },
            palette,
        );
        div()
            // Never squeeze below the frozen table's natural height (28px
            // pane head + 22px table header + 15×20px rows + 2px border):
            // shorter windows scroll the overview page instead of cutting
            // rows mid-glyph at the timeline boundary (spec gap 1).
            .min_h(px(352.))
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .h(px(metrics::PLOT_PANE_HEADER_HEIGHT))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(palette.surface_alt)
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(6.))
                            .min_w_0()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(13.))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(palette.muted)
                                    .child(self.localized("PVT 解算", "PVT solution")),
                            )
                            .child(div().text_size(px(11.)).text_color(palette.faint).child(
                                self.localized(
                                    "（最终选中解 · 选中 epoch）",
                                    "(final selected solution · selected epoch)",
                                ),
                            )),
                    )
                    .child(
                        GnssButton::icon_only(
                            "candidate-comparison",
                            IconName::ChartPie,
                            self.localized(
                                "打开 Raw、Smoothed 与 Kalman 候选解的详细比较",
                                "Compare Raw, Smoothed, and Kalman candidates",
                            ),
                            palette,
                        )
                        .disabled(pvt.is_none())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.workspace_view = WorkspaceView::Candidates;
                            this.side_drawer = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(table)
            .into_any_element()
    }

    fn render_run_configuration_rail(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let run = self.selected_run();
        let settings = run.map(|run| &run.settings).unwrap_or(&self.settings);
        let enabled = |value: bool| {
            if value {
                self.localized("启用", "On")
            } else {
                self.localized("关闭", "Off")
            }
        };
        let source = run
            .map(|run| run.source_path.as_path())
            .or(settings.iq_file_path.as_deref())
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "--".to_string());
        let nav_source = settings
            .rinex_nav_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                if settings.use_rinex_nav_environment {
                    "RINEX · ENV".to_string()
                } else if settings.auto_discover_colocated_nav {
                    self.localized("RINEX · 自动发现", "RINEX · Auto-discovery")
                        .to_string()
                } else {
                    self.localized("仅 IQ 导航电文", "IQ navigation only")
                        .to_string()
                }
            });
        let fixed_prns = settings
            .fixed_prns
            .as_ref()
            .map(|prns| {
                prns.iter()
                    .map(|prn| format!("G{prn:02}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| self.localized("自动", "Automatic").to_string());

        let view = ConfigurationView {
            title: self.localized("运行配置", "Run Configuration"),
            subtitle: if run.is_some() {
                self.localized("只读快照", "Read-only snapshot")
            } else {
                self.localized("草稿 · 在新建分析中编辑", "Draft · edit in New Analysis")
            },
            footer: run.is_some().then(|| {
                self.localized(
                    "修改配置需重新处理，将创建新的解算任务记录",
                    "Changing the configuration requires reprocessing and creates a new run",
                )
            }),
            entries: vec![
                ConfigurationEntry::Section(self.localized("数据源", "Source")),
                ConfigurationEntry::Row(self.localized("IQ 文件", "IQ file"), source),
                ConfigurationEntry::Row(
                    self.localized("格式", "Format"),
                    format!("{:?}", settings.iq_format),
                ),
                ConfigurationEntry::Row(
                    self.localized("信号", "Signal"),
                    "GPS · L1 C/A".to_string(),
                ),
                ConfigurationEntry::Section(self.localized("射频", "RF")),
                ConfigurationEntry::Row(
                    self.localized("采样率", "Sample rate"),
                    format!("{:.3} MHz", settings.sample_rate_hz / 1e6),
                ),
                ConfigurationEntry::Row(
                    self.localized("中频", "IF"),
                    format!("{:.3} MHz", settings.intermediate_frequency_hz / 1e6),
                ),
                ConfigurationEntry::Row(
                    self.localized("分块", "Chunk"),
                    format!("{} ms", settings.chunk_ms),
                ),
                ConfigurationEntry::Section(self.localized("捕获", "Acquisition")),
                ConfigurationEntry::Row(
                    self.localized("搜索窗", "Window"),
                    format!("{} ms", settings.acquisition_window_ms),
                ),
                ConfigurationEntry::Row(
                    self.localized("非相干积分", "Noncoherent"),
                    settings.acquisition_noncoherent_integrations.to_string(),
                ),
                ConfigurationEntry::Row(
                    self.localized("门限", "Threshold"),
                    format!("{:.2}", settings.acquisition_threshold),
                ),
                ConfigurationEntry::Row(
                    self.localized("多普勒范围", "Doppler span"),
                    format!("±{:.0} Hz", settings.acquisition_doppler_max_hz),
                ),
                ConfigurationEntry::Row(
                    self.localized("多普勒步长", "Doppler step"),
                    format!("{:.0} Hz", settings.acquisition_doppler_step_hz),
                ),
                ConfigurationEntry::Row(self.localized("PRN", "PRNs"), fixed_prns),
                ConfigurationEntry::Section(self.localized("跟踪", "Tracking")),
                ConfigurationEntry::Row("DLL BW", format!("{:.2} Hz", settings.dll_bandwidth_hz)),
                ConfigurationEntry::Row("PLL BW", format!("{:.2} Hz", settings.pll_bandwidth_hz)),
                ConfigurationEntry::Row(
                    self.localized("早迟间隔", "Early/late"),
                    format!("{:.2} chips", settings.early_late_spacing_chips),
                ),
                ConfigurationEntry::Row(
                    self.localized("载波辅助", "Carrier aiding"),
                    enabled(settings.carrier_aiding).to_string(),
                ),
                ConfigurationEntry::Section(self.localized("导航与 PVT", "Navigation & PVT")),
                ConfigurationEntry::Row(self.localized("导航源", "NAV source"), nav_source),
                ConfigurationEntry::Row(
                    "Hatch",
                    enabled(settings.enable_carrier_smoothing).to_string(),
                ),
                ConfigurationEntry::Row("Kalman", enabled(settings.enable_kalman).to_string()),
                ConfigurationEntry::Row("RAIM", enabled(settings.enable_raim).to_string()),
                ConfigurationEntry::Row(
                    self.localized("整毫秒模糊度", "Integer-ms ambiguity"),
                    enabled(settings.enable_integer_ms_ambiguity_resolution).to_string(),
                ),
                ConfigurationEntry::Section(self.localized("执行", "Execution")),
                ConfigurationEntry::Row(
                    self.localized("PVT 周期", "PVT cadence"),
                    format!("{} ms", settings.pvt_output_rate_ms),
                ),
                ConfigurationEntry::Row(
                    self.localized("并行捕获", "Parallel acquisition"),
                    enabled(settings.parallel_acquisition).to_string(),
                ),
                ConfigurationEntry::Row(
                    self.localized("并行跟踪", "Parallel tracking"),
                    enabled(settings.parallel_tracking).to_string(),
                ),
            ],
        };
        let close_action = GnssButton::icon_only(
            "close-run-configuration",
            IconName::PanelRightClose,
            self.localized("折叠运行配置", "Collapse run configuration"),
            palette,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.set_run_configuration_visibility(false);
            cx.notify();
        }));
        crate::ui::run_configuration::render(view, close_action.into_any_element(), palette)
    }

    fn render_navigation_signal_panel(&self, palette: Palette) -> AnyElement {
        let pvt = self.current_pvt();
        let channels = self.display_channels();
        let selected_prn = self
            .pinned_prn
            .or(self.hovered_prn)
            .or_else(|| {
                channels
                    .iter()
                    .find(|channel| channel.nav_locked || channel.tracking_locked)
                    .map(|channel| channel.prn)
            })
            .or_else(|| channels.first().map(|channel| channel.prn));
        let channel =
            selected_prn.and_then(|prn| channels.iter().find(|channel| channel.prn == prn));
        let geometry = selected_prn.and_then(|prn| {
            pvt.and_then(|pvt| pvt.satellites.iter().find(|satellite| satellite.prn == prn))
        });
        let epoch_number = self.selected_epoch.map(|index| index + 1).unwrap_or(0);
        let epoch_count = self.display_epochs().len();
        let selected_event_count = self
            .display_diagnostic_events()
            .iter()
            .filter(|event| selected_prn.is_none() || event.prn == selected_prn)
            .count();
        let last_selected_event = self
            .display_diagnostic_events()
            .iter()
            .rev()
            .find(|event| selected_prn.is_none() || event.prn == selected_prn);
        let signal_status = channel
            .map(|channel| {
                if channel.nav_locked {
                    self.localized("导航锁定", "NAV locked")
                } else if channel.tracking_locked {
                    self.localized("稳定跟踪", "Tracking")
                } else if channel.acquired {
                    self.localized("已捕获", "Acquired")
                } else {
                    self.localized("搜索", "Searching")
                }
            })
            .unwrap_or(self.localized("无遥测", "No telemetry"));
        let signal_color = channel
            .map(|channel| {
                if channel.nav_locked {
                    palette.healthy
                } else if channel.tracking_locked {
                    palette.accent
                } else if channel.acquired {
                    palette.warning
                } else {
                    palette.muted
                }
            })
            .unwrap_or(palette.muted);

        div()
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .h(px(42.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(self.localized("卫星导航详情", "Satellite navigation detail")),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .font_family(data_font_family())
                            .text_color(palette.muted)
                            .child(format!("Epoch {epoch_number} / {epoch_count}")),
                    ),
            )
            .child(
                div()
                    .px(px(14.))
                    .py(px(11.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_family(data_font_family())
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(signal_color)
                                    .child(
                                        selected_prn
                                            .map(|prn| format!("G{prn:02} · L1CA · NAV"))
                                            .unwrap_or_else(|| "GPS · L1CA".to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(8.))
                                    .text_color(signal_color)
                                    .child(div().size(px(5.)).rounded_full().bg(signal_color))
                                    .child(signal_status),
                            ),
                    )
                    .child(inspector_section(self.localized("信号", "Signal"), palette))
                    .child(inspector_row(
                        "C/N0",
                        channel
                            .map(|channel| format!("{:.1} dB-Hz", channel.cn0_db_hz))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("多普勒", "Doppler"),
                        channel
                            .map(|channel| format!("{:+.1} Hz", channel.doppler_hz))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_section(
                        self.localized("跟踪", "Tracking"),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("捕获", "Acquisition"),
                        channel
                            .map(|channel| {
                                if channel.acquired {
                                    self.localized("已捕获", "Acquired")
                                } else {
                                    self.localized("搜索中", "Searching")
                                }
                                .to_string()
                            })
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("载波跟踪", "Carrier tracking"),
                        channel
                            .map(|channel| {
                                if channel.tracking_locked {
                                    self.localized("锁定", "Locked")
                                } else {
                                    self.localized("未锁定", "Unlocked")
                                }
                                .to_string()
                            })
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("导航电文", "Navigation data"),
                        channel
                            .map(|channel| {
                                if channel.nav_locked {
                                    self.localized("同步", "Synchronized")
                                } else {
                                    self.localized("等待同步", "Awaiting sync")
                                }
                                .to_string()
                            })
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_section(
                        self.localized("几何与质量", "Geometry and quality"),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("伪距残差", "Residual"),
                        channel
                            .and_then(|channel| channel.residual_m)
                            .map(|residual| format!("{residual:+.3} m"))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("方位 / 仰角", "Azimuth / elevation"),
                        geometry
                            .map(|satellite| {
                                format!(
                                    "{:.1}° / {:.1}°",
                                    satellite.azimuth_deg, satellite.elevation_deg
                                )
                            })
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("测距标准差", "Range sigma"),
                        geometry
                            .map(|satellite| format!("{:.3} m", satellite.pseudorange_sigma_m))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("用于 PVT", "Used in PVT"),
                        geometry
                            .map(|satellite| {
                                if satellite.used_in_solution {
                                    self.localized("是", "Yes")
                                } else {
                                    self.localized("否", "No")
                                }
                                .to_string()
                            })
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        "RAIM",
                        geometry
                            .map(|satellite| {
                                if satellite.raim_excluded {
                                    self.localized("已剔除", "Excluded")
                                } else {
                                    self.localized("通过", "Passed")
                                }
                                .to_string()
                            })
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_section(
                        self.localized("定位解", "Position solution"),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("解类型", "Solution"),
                        pvt.map(|pvt| format!("{:?}", pvt.solution_kind))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("纬度", "Latitude"),
                        pvt.map(|pvt| format!("{:.7}°", pvt.latitude_deg))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("经度", "Longitude"),
                        pvt.map(|pvt| format!("{:.7}°", pvt.longitude_deg))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("高程", "Altitude"),
                        pvt.map(|pvt| format!("{:.3} m", pvt.altitude_m))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("使用卫星", "Satellites used"),
                        pvt.map(|pvt| pvt.quality.n_sats.to_string())
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        "HDOP",
                        pvt.map(|pvt| format!("{:.3}", pvt.quality.hdop))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("接收机钟差", "Receiver clock"),
                        pvt.map(|pvt| format!("{:+.3} µs", pvt.time.receiver_clock_bias_us))
                            .unwrap_or_else(|| "--".to_string()),
                        palette,
                    ))
                    .child(inspector_section(
                        self.localized("导航与事件", "Navigation and events"),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("事件数", "Events"),
                        selected_event_count.to_string(),
                        palette,
                    ))
                    .children(last_selected_event.map(|event| {
                        div()
                            .mt(px(7.))
                            .p(px(8.))
                            .bg(palette.surface_alt)
                            .border_1()
                            .border_color(palette.border)
                            .text_size(px(8.))
                            .font_family(data_font_family())
                            .text_color(palette.muted)
                            .child(format!(
                                "{} ms · {}{}\n{}",
                                event.now_ms,
                                event.kind.as_str(),
                                event
                                    .prn
                                    .map(|prn| format!(" · G{prn:02}"))
                                    .unwrap_or_default(),
                                event.detail
                            ))
                    }))
                    .child(inspector_section(
                        self.localized("星历快照", "Ephemeris snapshot"),
                        palette,
                    ))
                    .child(inspector_row(
                        self.localized("可用性", "Availability"),
                        self.localized("当前运行快照未暴露", "Not exposed by run snapshot")
                            .to_string(),
                        palette,
                    )),
            )
            .into_any_element()
    }

    fn render_channel_table(&self, palette: Palette) -> AnyElement {
        let channels = self.display_channels();
        div()
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .h(px(42.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(Self::section_title(
                        self.localized("通道诊断", "Channel diagnostics"),
                        palette,
                    )),
            )
            .child(channel_table_header(self.interface_language, palette))
            .children(
                channels
                    .iter()
                    .map(|channel| channel_table_row(channel, self.interface_language, palette)),
            )
            .children(channels.is_empty().then(|| {
                div()
                    .h(px(72.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(self.localized(
                        "当前 epoch 没有通道遥测",
                        "No channel telemetry for this epoch",
                    ))
            }))
            .into_any_element()
    }

    fn render_navigation_events(&self, palette: Palette) -> AnyElement {
        let events = self.display_diagnostic_events();
        div()
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .h(px(42.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(Self::section_title(
                        self.localized("接收机事件", "Receiver events"),
                        palette,
                    )),
            )
            .child(event_table_header(palette))
            .children(
                events
                    .iter()
                    .rev()
                    .take(200)
                    .map(|event| event_table_row(event, palette)),
            )
            .children(events.is_empty().then(|| {
                div()
                    .h(px(110.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(self.localized(
                        "运行开始后将在此记录捕获、失锁、重捕获和导航电文状态",
                        "Acquisition, loss, reacquisition, and NAV events appear here",
                    ))
            }))
            .into_any_element()
    }

    fn render_overview(
        &self,
        palette: Palette,
        compact: bool,
        viewport_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let epochs = self.display_epochs();
        let channels = self.display_channels();
        let epoch_channels = self.display_epoch_channels();
        let epoch_period_ms = self
            .selected_run()
            .map(|run| run.settings.pvt_output_rate_ms)
            .unwrap_or(self.settings.pvt_output_rate_ms);
        let pvt = self.current_pvt();
        let valid_pvt = pvt.filter(|pvt| pvt_position_is_displayable(pvt));
        let satellites = valid_pvt
            .map(|pvt| pvt.satellites.as_slice())
            .unwrap_or(&[]);
        let _ = viewport_height;
        // Real C/N0 history window for pane subtitles (never a story value).
        let cn0_history_s =
            crate::plots::cn0::history_seconds(epoch_channels.len(), epoch_period_ms);
        let cn0_subtitle = Some(SharedString::from(format!("（过去 {cn0_history_s} s）")));
        if let Some(focus) = self.focused_plot {
            let (title, subtitle, plot, extra_controls) = match focus {
                PlotFocus::Sky => (
                    "卫星天空图".to_string(),
                    Some(SharedString::from("（选中 epoch）")),
                    crate::plots::sky::render(
                        satellites,
                        channels,
                        self.hovered_prn,
                        self.pinned_prn,
                        palette,
                        cx,
                    ),
                    Vec::new(),
                ),
                PlotFocus::Position => (
                    "ENU 位置 (m)".to_string(),
                    Some(SharedString::from("（最终选中解）")),
                    crate::plots::enu::render(epochs, self.selected_epoch, palette),
                    Vec::new(),
                ),
                PlotFocus::Cn0 => (
                    // Unit lives on the axis title (issue 11 F2); the pane
                    // title carries the history role and the signal identity.
                    "C/N0 历史 · GPS L1 C/A".to_string(),
                    cn0_subtitle.clone(),
                    crate::plots::cn0::render(
                        epoch_channels,
                        channels,
                        crate::plots::cn0::Cn0PlotSelection {
                            epoch_period_ms,
                            selected_epoch: self.selected_epoch,
                            hovered_prn: self.hovered_prn,
                            pinned_prn: self.pinned_prn,
                        },
                        palette,
                        cx,
                    ),
                    vec![crate::plots::cn0::scale_control(palette)],
                ),
                PlotFocus::Velocity => (
                    "速度与钟漂".to_string(),
                    None,
                    velocity_plot(epochs, self.selected_epoch, palette).into_any_element(),
                    Vec::new(),
                ),
                PlotFocus::Altitude => (
                    "高度历史 (m)".to_string(),
                    None,
                    altitude_plot(epochs, self.selected_epoch, palette).into_any_element(),
                    Vec::new(),
                ),
            };
            return div()
                .px(px(12.))
                .pb(px(20.))
                .child(self.plot_panel(
                    title,
                    subtitle,
                    plot,
                    palette,
                    focus,
                    PlotPanelLayout::Expanded,
                    extra_controls,
                    cx,
                ))
                .into_any_element();
        }
        let content = match self.workspace_view {
            WorkspaceView::Overview => {
                // Build the four panes first (each borrows `cx` in turn), then
                // compose two explicit rows instead of an auto-sized 2x2 grid:
                // each row takes half the remaining height, so the panes always
                // fit between the workbench header and the fixed 148px timeline
                // at the reference sizes instead of overflowing it.
                let sky_pane = self.plot_panel(
                    "卫星天空图",
                    Some(SharedString::from("（选中 epoch）")),
                    crate::plots::sky::render(
                        satellites,
                        channels,
                        self.hovered_prn,
                        self.pinned_prn,
                        palette,
                        cx,
                    ),
                    palette,
                    PlotFocus::Sky,
                    PlotPanelLayout::Standard,
                    Vec::new(),
                    cx,
                );
                let enu_pane = self.plot_panel(
                    "ENU 位置 (m)",
                    Some(SharedString::from("（最终选中解）")),
                    crate::plots::enu::render(epochs, self.selected_epoch, palette),
                    palette,
                    PlotFocus::Position,
                    PlotPanelLayout::Standard,
                    Vec::new(),
                    cx,
                );
                let pvt_pane = self.render_pvt_summary(palette, false, cx);
                let cn0_pane = self.plot_panel(
                    "C/N0 历史 · GPS L1 C/A",
                    cn0_subtitle.clone(),
                    crate::plots::cn0::render(
                        epoch_channels,
                        channels,
                        crate::plots::cn0::Cn0PlotSelection {
                            epoch_period_ms,
                            selected_epoch: self.selected_epoch,
                            hovered_prn: self.hovered_prn,
                            pinned_prn: self.pinned_prn,
                        },
                        palette,
                        cx,
                    ),
                    palette,
                    PlotFocus::Cn0,
                    PlotPanelLayout::Standard,
                    vec![crate::plots::cn0::scale_control(palette)],
                    cx,
                );
                if compact {
                    // One column: panes keep their own height and the parent
                    // scrolls, so nothing is compressed into a sliver.
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(metrics::PLOT_GRID_GAP))
                        .child(div().h(px(332.)).child(sky_pane))
                        .child(div().h(px(332.)).child(enu_pane))
                        .child(div().h(px(332.)).child(pvt_pane))
                        .child(div().h(px(332.)).child(cn0_pane))
                        .into_any_element()
                } else {
                    div()
                        .h_full()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap(px(metrics::PLOT_GRID_GAP))
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .gap(px(metrics::PLOT_GRID_GAP))
                                .child(div().flex_1().min_w_0().child(sky_pane))
                                .child(div().flex_1().min_w_0().child(enu_pane)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .gap(px(metrics::PLOT_GRID_GAP))
                                .child(div().flex_1().min_w_0().child(pvt_pane))
                                .child(div().flex_1().min_w_0().child(cn0_pane)),
                        )
                        .into_any_element()
                }
            }
            WorkspaceView::Channels => div()
                // Fixed-height history pane above a table that owns its own
                // scroll region, so the last channel row is never clipped by
                // the timeline edge (design pass).
                .child(div().h(px(340.)).flex_shrink_0().child(self.plot_panel(
                    "稳定通道 C/N0 历史",
                    cn0_subtitle.clone(),
                    crate::plots::cn0::render(
                        epoch_channels,
                        channels,
                        crate::plots::cn0::Cn0PlotSelection {
                            epoch_period_ms,
                            selected_epoch: self.selected_epoch,
                            hovered_prn: self.hovered_prn,
                            pinned_prn: self.pinned_prn,
                        },
                        palette,
                        cx,
                    ),
                    palette,
                    PlotFocus::Cn0,
                    PlotPanelLayout::Standard,
                    vec![crate::plots::cn0::scale_control(palette)],
                    cx,
                )))
                .child(
                    div()
                        .mt(px(metrics::PLOT_GRID_GAP))
                        .child(self.render_channel_table(palette)),
                )
                .into_any_element(),
            WorkspaceView::Position => div()
                .child(self.plot_panel(
                    "ENU 位置 (m)",
                    Some(SharedString::from("（最终选中解）")),
                    crate::plots::enu::render(epochs, self.selected_epoch, palette),
                    palette,
                    PlotFocus::Position,
                    PlotPanelLayout::Standard,
                    Vec::new(),
                    cx,
                ))
                .child(
                    div()
                        .mt(px(6.))
                        .grid()
                        .grid_cols(if compact { 1 } else { 2 })
                        .gap(px(6.))
                        .child(self.plot_panel(
                            "速度与钟漂",
                            None,
                            velocity_plot(epochs, self.selected_epoch, palette),
                            palette,
                            PlotFocus::Velocity,
                            PlotPanelLayout::Standard,
                            Vec::new(),
                            cx,
                        ))
                        .child(self.plot_panel(
                            "高度历史 (m)",
                            None,
                            altitude_plot(epochs, self.selected_epoch, palette),
                            palette,
                            PlotFocus::Altitude,
                            PlotPanelLayout::Standard,
                            Vec::new(),
                            cx,
                        )),
                )
                .into_any_element(),
            WorkspaceView::Navigation => {
                if compact {
                    div()
                        .child(self.render_navigation_signal_panel(palette))
                        .child(
                            div()
                                .mt(px(6.))
                                .child(self.render_navigation_events(palette)),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex()
                        .items_start()
                        .gap(px(6.))
                        .child(
                            div()
                                .w(gpui_kit::relative(0.43))
                                .child(self.render_navigation_signal_panel(palette)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.render_navigation_events(palette)),
                        )
                        .into_any_element()
                }
            }
            WorkspaceView::Candidates => div()
                .grid()
                .grid_cols(if compact { 1 } else { 3 })
                .gap(px(6.))
                .children(
                    [
                        PvtEpochSolutionKind::Raw,
                        PvtEpochSolutionKind::Smoothed,
                        PvtEpochSolutionKind::Kalman,
                    ]
                    .into_iter()
                    .map(|kind| self.render_candidate_card(pvt, kind, palette)),
                )
                .into_any_element(),
        };

        // Plot grid gutter recovers height for the fixed 148px timeline
        // (design-refresh spec §3): outer padding 12/10/20 -> 4 and row gap
        // 6 -> 4. Charts keep explicit minimums and may scroll internally.
        div()
            .px(px(metrics::PLOT_GRID_PADDING))
            .pt(px(metrics::PLOT_GRID_PADDING))
            .pb(px(metrics::PLOT_GRID_PADDING))
            .child(content)
            .children(
                matches!(self.workspace_view, WorkspaceView::Position).then(|| {
                    div()
                        .mt(px(6.))
                        .flex_shrink_0()
                        .child(self.render_pvt_summary(palette, compact, cx))
                }),
            )
            .into_any_element()
    }

    fn render_candidate_drawer(
        &self,
        palette: Palette,
        compact: bool,
        full_width: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pvt = self.current_pvt();
        let epoch_number = self.selected_epoch.map(|index| index + 1).unwrap_or(0);
        let epoch_count = self.display_epochs().len();
        div()
            .when(full_width, |this| this.w_full().flex_1())
            .when(!full_width, |this| {
                this.w(if compact { px(350.) } else { px(430.) })
                    .flex_shrink_0()
            })
            .h_full()
            .overflow_y_scrollbar()
            .bg(palette.surface)
            .border_l_1()
            .border_color(palette.border)
            .shadow_lg()
            .p(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                    .text_color(palette.text)
                                    .child("候选解比较"),
                            )
                            .child(
                                div()
                                    .mt(px(3.))
                                    .text_size(px(10.))
                                    .text_color(palette.muted)
                                    .child(format!("Epoch {epoch_number} / {epoch_count}")),
                            ),
                    )
                    .child(
                        GnssButton::icon_only(
                            "close-candidate-comparison",
                            IconName::CircleX,
                            self.localized("关闭候选解比较", "Close candidate comparison"),
                            palette,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.side_drawer = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.))
                    .text_color(palette.muted)
                    .child("只读诊断 · 最终解由接收机质量门限与优先级自动选择"),
            )
            .child(
                div()
                    .mt(px(10.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.surface)
                    .flex()
                    .items_stretch()
                    .children(
                        [
                            PvtEpochSolutionKind::Raw,
                            PvtEpochSolutionKind::Smoothed,
                            PvtEpochSolutionKind::Kalman,
                        ]
                        .into_iter()
                        .map(|kind| self.render_candidate_card(pvt, kind, palette)),
                    ),
            )
            .into_any_element()
    }

    fn render_candidate_card(
        &self,
        pvt: Option<&GuiPvtViewModel>,
        kind: PvtEpochSolutionKind,
        palette: Palette,
    ) -> impl IntoElement {
        let candidate = pvt.and_then(|pvt| pvt.candidate(kind));
        let disposition = pvt
            .map(|pvt| pvt.candidate_disposition(kind))
            .unwrap_or(PvtCandidateDisposition::Unavailable);
        let status_color = candidate_disposition_color(disposition, palette);
        let position_delta_m = pvt.zip(candidate).map(|(pvt, candidate)| {
            ((candidate.ecef_m[0] - pvt.ecef_m[0]).powi(2)
                + (candidate.ecef_m[1] - pvt.ecef_m[1]).powi(2)
                + (candidate.ecef_m[2] - pvt.ecef_m[2]).powi(2))
            .sqrt()
        });
        let clock_drift_ppm = candidate
            .and_then(|candidate| candidate.velocity.as_ref())
            .map(|velocity| velocity.clock_drift_mps / 299_792_458.0 * 1e6);
        // Candidate columns read as one instrument split by hairlines, not as
        // three floating cards; the adopted candidate is marked by a 2px accent
        // edge, and its disposition is bare colored text (no filled chip).
        div()
            .flex_1()
            .min_w_0()
            .p(px(14.))
            .when(disposition == PvtCandidateDisposition::Selected, |this| {
                this.border_l_2()
                    .border_color(palette.accent)
                    .bg(palette.accent.opacity(0.04))
            })
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(candidate_kind_label(kind)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(status_color)
                            .child(candidate_disposition_label(disposition)),
                    ),
            )
            .children(candidate.map(|candidate| {
                div()
                    .mt(px(10.))
                    .child(comparison_group_title("位置", palette))
                    .child(comparison_metric(
                        "纬度 / 经度",
                        format!(
                            "{:.7}° / {:.7}°",
                            candidate.latitude_deg, candidate.longitude_deg
                        ),
                        palette,
                    ))
                    .child(comparison_metric(
                        "高度",
                        format!("{:.3} m", candidate.altitude_m),
                        palette,
                    ))
                    .child(comparison_metric(
                        "相对最终解",
                        format_optional_metric(position_delta_m, "m", 3),
                        palette,
                    ))
                    .child(comparison_group_title("速度与时钟", palette))
                    .child(comparison_metric(
                        "三维 / 水平速度",
                        candidate
                            .velocity
                            .as_ref()
                            .map(|velocity| {
                                format!(
                                    "{:.4} / {:.4} m/s",
                                    velocity.speed_mps, velocity.horizontal_speed_mps
                                )
                            })
                            .unwrap_or_else(|| "不可用".to_string()),
                        palette,
                    ))
                    .child(comparison_metric(
                        "垂直速度",
                        candidate
                            .velocity
                            .as_ref()
                            .map(|velocity| format!("{:.4} m/s", velocity.vertical_speed_mps))
                            .unwrap_or_else(|| "不可用".to_string()),
                        palette,
                    ))
                    .child(comparison_metric(
                        "钟差 / 钟漂",
                        format!(
                            "{:.3} µs / {}",
                            candidate.receiver_clock_bias_us,
                            format_optional_metric(clock_drift_ppm, "ppm", 5)
                        ),
                        palette,
                    ))
                    .child(comparison_group_title("质量", palette))
                    .child(comparison_metric(
                        "卫星 / HDOP",
                        format!(
                            "{} / {:.3}",
                            candidate.quality.n_sats, candidate.quality.hdop
                        ),
                        palette,
                    ))
                    .child(comparison_metric(
                        "残差 RMS",
                        format!("{:.3} m", candidate.quality.residual_rms_m),
                        palette,
                    ))
                    .child(comparison_metric(
                        "水平 / 垂直 σ",
                        format!(
                            "{:.3} / {:.3} m",
                            candidate.quality.horizontal_sigma_m,
                            candidate.quality.vertical_sigma_m
                        ),
                        palette,
                    ))
                    .child(comparison_metric(
                        "使用 PRN",
                        format_prns(&candidate.quality.used_prns),
                        palette,
                    ))
            }))
            .children(candidate.is_none().then(|| {
                div()
                    .mt(px(10.))
                    .py(px(8.))
                    .text_size(px(10.))
                    .text_color(palette.muted)
                    .child(candidate_empty_message(disposition))
            }))
    }

    fn render_timeline_bar(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let epochs = self.display_epochs();
        let events = self.display_diagnostic_events();
        let epoch_count = epochs.len();
        let epoch_period_ms = self
            .selected_run()
            .map(|run| run.settings.pvt_output_rate_ms)
            .unwrap_or(self.settings.pvt_output_rate_ms);
        let epoch_duration_ms = epoch_count.saturating_sub(1) as u64 * epoch_period_ms;
        let timeline_duration_ms = events
            .last()
            .map(|event| event.now_ms)
            .unwrap_or(0)
            .max(epoch_duration_ms)
            .max(1);
        let epoch_number = self.selected_epoch.map(|index| index + 1).unwrap_or(0);
        let global_cursor = self.timeline_cursor_fraction();
        let (visible_start, visible_span) = self.timeline_window();
        let cursor_percent = ((global_cursor - visible_start) / visible_span).clamp(0.0, 1.0);
        // Real processed progress of the selected run; completed runs render a
        // full processed block with no boundary marker (preview active run is
        // the deterministic 0.63 fixture).
        let processed = self
            .selected_run()
            .map(|run| (run.progress as f32).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        // Real summary subtitle: receiver-time span, PVT output rate, events.
        let receiver_seconds = timeline_duration_ms / 1_000;
        let pvt_hz = if epoch_period_ms > 0 && 1_000 % epoch_period_ms == 0 {
            format!("{}", 1_000 / epoch_period_ms)
        } else {
            format!("{:.1}", 1_000.0 / epoch_period_ms.max(1) as f64)
        };
        let (time_label, event_label) = match self.interface_language {
            InterfaceLanguage::SimplifiedChinese => ("接收机时间", "事件"),
            InterfaceLanguage::English => ("Receiver time", "Events"),
        };
        let subtitle = format!(
            "{time_label} {receiver_seconds} s · PVT {pvt_hz} Hz · {event_label} {}",
            events.len()
        );
        // Real amplitude envelope for the minimap: per-epoch peak C/N0 across
        // the selected run's channels. Empty channels stay `None` so the
        // minimap draws the 不可用 hatch instead of a synthetic waveform.
        let peak_per_epoch = self
            .display_epoch_channels()
            .iter()
            .map(|channels| {
                let peak = channels
                    .iter()
                    .map(|channel| channel.cn0_db_hz)
                    .filter(|cn0| cn0.is_finite() && *cn0 > 0.0)
                    .fold(0.0_f64, f64::max);
                (peak > 0.0).then_some(peak as f32)
            })
            .collect::<Vec<_>>();
        let minimap_envelope = crate::plots::timeline::envelope_series(
            &peak_per_epoch,
            epoch_period_ms.max(1),
            timeline_duration_ms,
            crate::plots::timeline::ENVELOPE_BUCKETS,
        );
        let track = crate::plots::timeline::TrackView {
            duration_ms: timeline_duration_ms,
            visible_start,
            visible_span,
            cursor_percent,
            epoch_number,
            processed,
            events: events
                .iter()
                .map(crate::plots::timeline::track_event)
                .collect(),
        };
        // Behavior-only seek slider: the timeline draws the visible playhead
        // itself, so the stock bar and thumb stay transparent while the
        // interactive track fills the whole timeline track. Grabbing the
        // playhead cap or its callout must drag exactly like grabbing the
        // baseline, not just the thin strip through the middle.
        let slider = gpui_kit::base::Slider::new(&self.timeline)
            .disabled(epoch_count < 2)
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .child(
                gpui_kit::base::SliderTrack::new(&self.timeline)
                    .disabled(epoch_count < 2)
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(
                        gpui_kit::base::SliderIndicator::new(&self.timeline)
                            .relative()
                            .w_full()
                            .h_full()
                            .bg(gpui_kit::transparent_black()),
                    ),
            );
        let localize: crate::ui::Localize = match self.interface_language {
            InterfaceLanguage::SimplifiedChinese => |zh, _| zh,
            InterfaceLanguage::English => |_, en| en,
        };
        let app = cx.entity();
        let actions: crate::ui::epoch_dock::EpochHandler =
            std::rc::Rc::new(move |action, window, cx| {
                app.update(cx, |this, cx| {
                    use crate::ui::epoch_dock::EpochAction;
                    match action {
                        EpochAction::First => this.jump_selected_epoch(0, window, cx),
                        EpochAction::Previous => this.step_selected_epoch(-1, window, cx),
                        EpochAction::Next => this.step_selected_epoch(1, window, cx),
                        EpochAction::Last => {
                            let last = this.display_epochs().len().saturating_sub(1);
                            this.jump_selected_epoch(last, window, cx);
                        }
                        EpochAction::ZoomOut => this.adjust_timeline_zoom(false),
                        EpochAction::ZoomIn => this.adjust_timeline_zoom(true),
                        EpochAction::ZoomReset => this.timeline_zoom = 1,
                        EpochAction::Follow => this.return_to_live(window, cx),
                        EpochAction::PreciseJump => this.begin_epoch_jump(window, cx),
                    }
                    cx.notify();
                });
            });
        let jump_input = self.epoch_jump_editing.then(|| {
            Input::new(&self.epoch_jump_input)
                .w_full()
                .bg(palette.surface.opacity(0.0))
                .border_color(palette.surface.opacity(0.0))
                .text_color(palette.accent)
                .font_family(data_font_family())
                .text_size(px(12.))
                .into_any_element()
        });
        crate::ui::epoch_dock::render(
            crate::ui::epoch_dock::EpochView {
                title: self.localized("选中解算任务时间线", "Selected run timeline"),
                subtitle,
                epoch_number,
                track,
                minimap_envelope,
                jump_editing: self.epoch_jump_editing,
                jump_input,
                seek: slider.into_any_element(),
                controls: crate::ui::epoch_dock::EpochControls {
                    epoch_count,
                    zoom: self.timeline_zoom,
                    follow_live: self.follow_live,
                    can_follow: self.live_run_id.is_some(),
                },
                actions,
                localize,
            },
            palette,
        )
    }

    fn render_status_bar(
        &self,
        palette: Palette,
        runs_dock_open: bool,
        runs_dock_replaces_content: bool,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.active_run.as_ref();
        // Middle segment shows the SELECTED run's current selected epoch, not
        // the live worker state (issue 17 item 3): browsing history while
        // another run processes must not leak live counts/settings.
        let display_channels = self.display_channels();
        let tracking_count = display_channels
            .iter()
            .filter(|channel| channel.tracking_locked)
            .count();
        let nav_count = display_channels
            .iter()
            .filter(|channel| channel.nav_locked)
            .count();
        // Sample rate comes from the selected run's persisted snapshot; the
        // editor settings are only a fallback when no run is selected.
        let rate_hz = self
            .selected_run()
            .map(|run| run.settings.sample_rate_hz)
            .unwrap_or(self.settings.sample_rate_hz);
        // Honest fix type: the real 2D/3D fix derived from the selected
        // epoch's outputs, or the unavailable fallback. The candidate kind
        // (Raw/Smoothed/Kalman) is never presented as a fix type, and an
        // unconverged selection is never colored healthy (spec §2.5).
        let (fix_label, fix_color) = match self.current_pvt().and_then(|pvt| pvt.fix_type()) {
            Some(fix) => (fix.label().to_string(), palette.healthy),
            None => (
                self.localized("不可用", "Unavailable").to_string(),
                palette.muted,
            ),
        };
        let active_label = active
            .map(|run| {
                format!(
                    "{} {:.0}%",
                    self.localized("处理中", "Running"),
                    run.progress * 100.0
                )
            })
            .unwrap_or_else(|| "Idle".to_string());
        let toggle = GnssButton::icon_only(
            "toggle-runs-sidebar",
            if runs_dock_open {
                IconName::PanelLeftClose
            } else {
                IconName::PanelLeftOpen
            },
            if runs_dock_open {
                self.localized("折叠运行栏", "Collapse Runs dock")
            } else {
                self.localized("展开运行栏", "Expand Runs dock")
            },
            palette,
        )
        .size(ControlSize::Compact)
        .rounded(px(0.))
        .on_click(cx.listener(move |this, _, _, cx| {
            if runs_dock_replaces_content {
                this.compact_library_expanded = !this.compact_library_expanded;
            } else {
                this.set_runs_sidebar_visibility(!this.runs_sidebar_visible);
            }
            cx.notify();
        }));
        // A selection without any channel/PVT data renders "—" (the strip's
        // empty-value fallback) instead of a fabricated 0 (spec §7).
        let selected_context = !display_channels.is_empty() || self.current_pvt().is_some();
        let (tracking_value, nav_value) = if selected_context {
            (tracking_count.to_string(), nav_count.to_string())
        } else {
            (String::new(), String::new())
        };
        crate::ui::status_strip::render(
            palette,
            toggle.into_any_element(),
            [
                (
                    self.localized("活动运行", "Active run"),
                    active_label,
                    if active.is_some() {
                        palette.healthy
                    } else {
                        palette.muted
                    },
                ),
                ("SIGNAL", "GPS L1 C/A".to_string(), palette.text),
                (
                    "RATE",
                    if selected_context {
                        format!("{:.3} MHz", rate_hz / 1e6)
                    } else {
                        String::new()
                    },
                    palette.text,
                ),
                ("TRACK", tracking_value, palette.text),
                ("NAV", nav_value, palette.text),
                ("FIX", fix_label, fix_color),
            ],
            self.status_message.clone(),
            compact,
        )
    }

    // Pane composition takes title/subtitle/plot/controls plus focus state;
    // eight positional arguments match the existing section-helper style.
    #[allow(clippy::too_many_arguments)]
    fn plot_panel(
        &self,
        title: impl Into<SharedString>,
        subtitle: Option<SharedString>,
        plot: impl IntoElement,
        palette: Palette,
        focus: PlotFocus,
        layout: PlotPanelLayout,
        extra_controls: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let expanded = matches!(layout, PlotPanelLayout::Expanded);
        let focus_control = GnssButton::icon_only(
            SharedString::from(format!("plot-focus-{}", focus.id())),
            if expanded {
                IconName::Minimize
            } else {
                IconName::Maximize
            },
            if expanded {
                self.localized("退出聚焦", "Exit focus")
            } else {
                self.localized("聚焦图表", "Focus plot")
            },
            palette,
        )
        .size(ControlSize::Compact)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.workspace_view = WorkspaceView::for_plot(focus);
            this.focused_plot = if expanded { None } else { Some(focus) };
            cx.notify();
        }));
        let mut controls = extra_controls;
        controls.push(focus_control.into_any_element());
        crate::plots::frame::render(
            title,
            subtitle,
            plot.into_any_element(),
            controls,
            expanded,
            palette,
        )
    }
}

impl Drop for GnsscopeApp {
    fn drop(&mut self) {
        self.worker_receiver.take();
        self.worker.take();
        self.persistence_writer.take();
    }
}

impl Render for GnsscopeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette();
        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        if self.component_gallery {
            return self.render_component_gallery(palette, cx);
        }
        let shell_layout = ShellLayout::resolve(
            viewport_width,
            self.side_drawer.is_some(),
            self.run_configuration_visible,
            self.page == AppPage::Workspace,
            self.runs_sidebar_visible,
            self.compact_library_expanded,
        );
        let compact = shell_layout.compact;
        let workspace_empty = self.selected_run_id.is_none()
            && self.active_run.is_none()
            && self.display_epochs().is_empty();
        let page_content = match self.page {
            // Document pages live inside one floating panel so they share the
            // workbench chassis (rounded surface + the single soft shadow).
            AppPage::NewAnalysis => crate::ui::shell::document_panel(
                self.render_new_analysis_content(palette, compact, cx),
                palette,
            ),
            AppPage::RunLibrary => crate::ui::shell::document_panel(
                self.render_run_library_content(palette, compact, cx),
                palette,
            ),
            AppPage::InterfaceSettings => crate::ui::shell::document_panel(
                self.render_interface_settings_content(palette, cx),
                palette,
            ),
            AppPage::Workspace => {
                let workspace =
                    shell_layout.show_workspace.then(|| {
                        if workspace_empty {
                            self.render_workspace_empty(palette, cx)
                        } else {
                            div()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .h_full()
                                .flex()
                                .flex_col()
                                .child(self.render_workspace_tabs(palette, compact, cx))
                                .child(div().flex_1().min_h_0().overflow_y_scrollbar().child(
                                    self.render_overview(palette, compact, viewport_height, cx),
                                ))
                                .into_any_element()
                        }
                    });
                let side_region = match self.side_drawer {
                    Some(SideDrawer::Settings) => Some(
                        self.render_config_drawer(
                            palette,
                            compact,
                            shell_layout.drawer_replaces_workspace,
                            cx,
                        )
                        .into_any_element(),
                    ),
                    Some(SideDrawer::CandidateComparison) => Some(
                        self.render_candidate_drawer(
                            palette,
                            compact,
                            shell_layout.drawer_replaces_workspace,
                            cx,
                        )
                        .into_any_element(),
                    ),
                    None => shell_layout
                        .show_run_configuration
                        .then(|| self.render_run_configuration_rail(palette, cx)),
                };
                let timeline = (self.timeline_visible && !workspace_empty)
                    .then(|| self.render_timeline_bar(palette, cx).into_any_element());
                workspace_shell(palette, workspace, side_region, timeline)
            }
        };
        let runs_dock = shell_layout
            .show_runs_dock
            .then(|| self.render_runs_dock(palette, shell_layout.runs_dock_replaces_content, cx));
        let body = shell_body(
            palette,
            runs_dock,
            shell_layout.show_content.then_some(page_content),
        );
        let root = shell_root(
            palette,
            self.render_title_bar(palette, compact, cx)
                .into_any_element(),
            body,
            self.render_status_bar(
                palette,
                shell_layout.show_runs_dock,
                shell_layout.runs_dock_replaces_content,
                compact,
                cx,
            )
            .into_any_element(),
        );
        // ⌘K palette overlay (issue 17 item 1): a floating layer anchored just
        // under the title bar, horizontally centered on a 420px column. The
        // shell stays the layout root; the overlay is a sibling it never
        // reflows.
        if self.command_palette_open {
            let view = self.palette_view(window, cx);
            let app = cx.entity();
            let on_select: CommandPaletteSelect = std::rc::Rc::new(move |index, window, cx| {
                app.update(cx, |this, cx| {
                    let command = this.palette_filtered_commands(cx).get(index).copied();
                    if let Some(command) = command {
                        this.close_command_palette(window, cx);
                        this.execute_palette_command(command, window, cx);
                    }
                });
            });
            let palette_width = crate::ui::overlays::OVERLAY_WIDTH;
            let left = ((viewport_width - palette_width) / 2.0).max(0.0);
            div()
                .size_full()
                .relative()
                .child(root)
                .child(
                    div()
                        .absolute()
                        .top(px(metrics::TITLE_BAR_HEIGHT + 4.0))
                        .left(px(left))
                        .child(crate::ui::overlays::command_palette(
                            &view, palette, on_select,
                        )),
                )
                .into_any_element()
        } else {
            root
        }
    }
}

pub(crate) fn format_timeline_time(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

/// Parse the precise jump field: a 1-based epoch number clamped into the
/// selected run's epoch range. Empty input, `0`, or a run without epochs all
/// mean "no jump"; oversized values land on the newest available epoch rather
/// than failing the edit.
fn epoch_jump_index(value: &str, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let number: usize = value.trim().parse().ok()?;
    if number == 0 {
        return None;
    }
    Some((number - 1).min(count - 1))
}

fn processing_run_elapsed_ms(run: &ProcessingRun) -> u64 {
    if !run.settings.sample_rate_hz.is_finite() || run.settings.sample_rate_hz <= 0.0 {
        return 0;
    }
    ((run.processed_samples as f64 / run.settings.sample_rate_hz) * 1_000.0) as u64
}

fn table_cell(
    value: impl Into<SharedString>,
    width: f32,
    palette: Palette,
    muted: bool,
) -> impl IntoElement {
    table_cell_aligned(value, width, palette, muted, TextAlign::Left)
}

/// One table cell; `align` lets a numeric column right-align on the decimal
/// side while keeping one shared cell builder for padding and typography.
fn table_cell_aligned(
    value: impl Into<SharedString>,
    width: f32,
    palette: Palette,
    muted: bool,
    align: TextAlign,
) -> impl IntoElement {
    div()
        .w(px(width))
        .flex_shrink_0()
        .when(align == TextAlign::Right, |this| this.text_right())
        .text_size(px(10.))
        .font_family(data_font_family())
        .text_color(if muted { palette.muted } else { palette.text })
        .child(value.into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextAlign {
    Left,
    Right,
}

fn channel_table_header(language: InterfaceLanguage, palette: Palette) -> impl IntoElement {
    let zh = language == InterfaceLanguage::SimplifiedChinese;
    div()
        .h(px(metrics::TABLE_HEADER_HEIGHT))
        .px(px(12.))
        .flex()
        .items_center()
        .bg(palette.surface_alt)
        .border_b_1()
        .border_color(palette.border)
        .child(table_cell(
            if zh { "卫星" } else { "SV" },
            64.0,
            palette,
            true,
        ))
        .child(table_cell(
            if zh { "状态" } else { "State" },
            112.0,
            palette,
            true,
        ))
        .child(table_cell_aligned(
            "C/N0 (dB-Hz)",
            104.0,
            palette,
            true,
            TextAlign::Right,
        ))
        .child(table_cell_aligned(
            if zh { "多普勒 (Hz)" } else { "Doppler (Hz)" },
            108.0,
            palette,
            true,
            TextAlign::Right,
        ))
        .child(table_cell_aligned(
            if zh { "残差 (m)" } else { "Residual (m)" },
            104.0,
            palette,
            true,
            TextAlign::Right,
        ))
}

fn channel_table_row(
    channel: &GuiChannelStatus,
    language: InterfaceLanguage,
    palette: Palette,
) -> impl IntoElement {
    let zh = language == InterfaceLanguage::SimplifiedChinese;
    let (state, state_color) = if channel.nav_locked {
        (if zh { "导航锁定" } else { "NAV lock" }, palette.healthy)
    } else if channel.tracking_locked {
        (if zh { "跟踪中" } else { "Tracking" }, palette.accent)
    } else if channel.acquired {
        (if zh { "已捕获" } else { "Acquired" }, palette.warning)
    } else {
        (if zh { "搜索中" } else { "Searching" }, palette.muted)
    };
    div()
        .h(px(metrics::TABLE_ROW_HEIGHT))
        .px(px(12.))
        .flex()
        .items_center()
        .border_b_1()
        .border_color(palette.border.opacity(0.55))
        .child(
            div()
                .w(px(64.))
                .flex_shrink_0()
                .text_size(px(11.))
                .font_family(data_font_family())
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(format!("G{:02}", channel.prn)),
        )
        .child(
            div()
                .w(px(112.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(7.))
                // Shape (filled / half / hollow / dot) plus the word: state is
                // never carried by color alone (spec §1.3).
                .child(
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .bg(if channel.nav_locked || channel.tracking_locked {
                            state_color
                        } else {
                            palette.surface.opacity(0.0)
                        })
                        .border_1()
                        .border_color(state_color),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(state_color)
                        .child(state),
                ),
        )
        .child(table_cell_aligned(
            format!("{:.1}", channel.cn0_db_hz),
            104.0,
            palette,
            false,
            TextAlign::Right,
        ))
        .child(table_cell_aligned(
            format!("{:+.0}", channel.doppler_hz),
            108.0,
            palette,
            false,
            TextAlign::Right,
        ))
        .child(table_cell_aligned(
            channel
                .residual_m
                .map(|residual| format!("{residual:+.3}"))
                .unwrap_or_else(|| "--".to_string()),
            104.0,
            palette,
            channel.residual_m.is_none(),
            TextAlign::Right,
        ))
}

fn event_kind_label(event: &GuiDiagnosticEvent) -> &'static str {
    match event.kind.as_str() {
        "acquisition" => "ACQUIRED",
        "acquisition_failed" => "ACQ FAILED",
        "reacquisition" => "REACQUIRED",
        "reacquisition_failed" => "REACQ FAILED",
        "channel_state" => "CHANNEL",
        "navigation" => "NAV",
        _ => "EVENT",
    }
}

fn event_kind_color(event: &GuiDiagnosticEvent, palette: Palette) -> Hsla {
    match event.kind.as_str() {
        "acquisition" | "reacquisition" | "navigation" => palette.healthy,
        "acquisition_failed" | "reacquisition_failed" => palette.danger,
        _ => palette.accent,
    }
}

fn preview_epoch_history() -> (Vec<GuiPvtViewModel>, Vec<Vec<GuiChannelStatus>>) {
    let base_epoch = crate::sample_pvt_view_model();
    let mut base_channels = crate::state::sample_channel_statuses();
    let template = base_channels
        .first()
        .cloned()
        .expect("sample preview must provide at least one channel");
    // Preview story (spec §3.0): nine tracked channels, seven of them
    // navigation-locked — gives the C/N0 pane its full nine-lane grid and the
    // sky plot seven PVT rings. Story values exist only under preview mode.
    base_channels.extend(
        [
            (3, 42.6, -1_253.7, Some(0.024), true),
            (8, 44.8, 2_180.0, Some(-0.031), true),
            (22, 39.5, -3_420.0, Some(0.087), false),
            (5, 41.2, -1_100.0, Some(0.045), false),
        ]
        .into_iter()
        .map(
            |(prn, cn0_db_hz, doppler_hz, residual_m, nav_locked)| GuiChannelStatus {
                prn,
                cn0_db_hz,
                doppler_hz,
                residual_m,
                nav_locked,
                ..template.clone()
            },
        ),
    );
    let epoch_count = 769;
    let latitude_scale_m = 111_111.0;
    let longitude_scale_m = latitude_scale_m * base_epoch.latitude_deg.to_radians().cos();
    let preview_azimuths = [338.0, 258.0, 112.0, 24.0, 198.0, 78.0, 146.0, 304.0, 210.0];
    let preview_elevations = [68.0, 42.0, 31.0, 53.0, 24.0, 61.0, 37.0, 47.0, 29.0];
    let epochs = (0..epoch_count)
        .map(|index| {
            let phase = index as f64 * 0.055;
            let (east_m, north_m) = preview_enu_position(index, epoch_count);
            let mut epoch = base_epoch.clone();
            epoch.epoch_index = index;
            epoch.latitude_deg += north_m / latitude_scale_m;
            epoch.longitude_deg += east_m / longitude_scale_m;
            epoch.altitude_m += (phase * 0.45).sin() * 1.8;
            if let Some(velocity) = &mut epoch.velocity {
                velocity.speed_mps = 2.6 + (phase * 0.8).sin() * 0.25;
                velocity.horizontal_speed_mps = velocity.speed_mps;
            }
            epoch.satellites = base_channels
                .iter()
                .enumerate()
                .map(|(satellite_index, channel)| {
                    let satellite_phase = index as f64 * 0.012 + satellite_index as f64 * 1.17;
                    GuiSatelliteGeometry {
                        prn: channel.prn,
                        azimuth_deg: (preview_azimuths[satellite_index]
                            + index as f64 * (0.06 + satellite_index as f64 * 0.006))
                            % 360.0,
                        elevation_deg: preview_elevations[satellite_index]
                            + satellite_phase.sin() * 3.5,
                        residual_m: channel.residual_m.unwrap_or(0.0),
                        standardized_residual: channel.residual_m.unwrap_or(0.0) / 1.5,
                        pseudorange_sigma_m: 1.2 + satellite_index as f64 * 0.18,
                        used_in_solution: channel.nav_locked,
                        raim_excluded: false,
                    }
                })
                .collect();
            epoch
        })
        .collect::<Vec<_>>();
    let channels = (0..epochs.len())
        .map(|index| {
            base_channels
                .iter()
                .cloned()
                .map(|mut channel| {
                    let phase = index as f64 * 0.075 + channel.prn as f64 * 0.43;
                    let noise = preview_noise(index, channel.prn) * 0.55
                        + preview_noise(index / 3, channel.prn + 41) * 0.22;
                    channel.cn0_db_hz +=
                        (phase * 0.11).sin() * 0.42 + (phase * 0.037).cos() * 0.24 + noise;
                    channel.doppler_hz += (phase * 0.2).sin() * 18.0;
                    channel
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    (epochs, channels)
}

fn preview_noise(index: usize, salt: u8) -> f64 {
    let mut value = (index as u64 + 1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((salt as u64 + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    value as f64 / u64::MAX as f64 * 2.0 - 1.0
}

fn preview_enu_position(index: usize, count: usize) -> (f64, f64) {
    const CONTROL_POINTS: [(f64, f64); 8] = [
        (0.0, 0.0),
        (-42.0, -14.0),
        (-20.0, -24.0),
        (-14.0, 12.0),
        (42.0, 6.0),
        (24.0, 0.0),
        (17.0, -15.0),
        (20.0, 1.0),
    ];
    if count <= 1 {
        return CONTROL_POINTS[0];
    }
    let scaled =
        index.min(count - 1) as f64 / (count - 1) as f64 * (CONTROL_POINTS.len() - 1) as f64;
    let segment = (scaled.floor() as usize).min(CONTROL_POINTS.len() - 2);
    let t = scaled - segment as f64;
    let p0 = CONTROL_POINTS[segment.saturating_sub(1)];
    let p1 = CONTROL_POINTS[segment];
    let p2 = CONTROL_POINTS[segment + 1];
    let p3 = CONTROL_POINTS[(segment + 2).min(CONTROL_POINTS.len() - 1)];
    let interpolate = |a: f64, b: f64, c: f64, d: f64| {
        let t2 = t * t;
        let t3 = t2 * t;
        0.5 * ((2.0 * b)
            + (-a + c) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
            + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (
        interpolate(p0.0, p1.0, p2.0, p3.0),
        interpolate(p0.1, p1.1, p2.1, p3.1),
    )
}

fn preview_diagnostic_events() -> Vec<GuiDiagnosticEvent> {
    [
        (
            1_200,
            Some(1),
            GuiDiagnosticEventKind::Acquisition,
            "Doppler +7000 Hz · metric 4.82",
        ),
        (
            95_000,
            Some(17),
            GuiDiagnosticEventKind::Acquisition,
            "Doppler -2200 Hz · metric 4.46",
        ),
        (
            182_000,
            Some(32),
            GuiDiagnosticEventKind::Acquisition,
            "Doppler +3100 Hz · metric 4.19",
        ),
        (
            260_000,
            Some(1),
            GuiDiagnosticEventKind::ChannelState,
            "PullIn -> TrackingLocked",
        ),
        (
            370_000,
            Some(11),
            GuiDiagnosticEventKind::ChannelState,
            "PullIn -> TrackingLocked",
        ),
        (
            510_000,
            Some(1),
            GuiDiagnosticEventKind::Navigation,
            "Subframe synchronization established",
        ),
        (
            548_000,
            Some(20),
            GuiDiagnosticEventKind::Navigation,
            "Ephemeris decoded",
        ),
        (
            680_000,
            Some(11),
            GuiDiagnosticEventKind::Reacquisition,
            "Doppler +5400 Hz · metric 4.31",
        ),
        (
            742_000,
            Some(24),
            GuiDiagnosticEventKind::AcquisitionFailed,
            "Peak metric 1.72 below threshold",
        ),
    ]
    .into_iter()
    .map(|(now_ms, prn, kind, detail)| GuiDiagnosticEvent {
        now_ms,
        sample_index: now_ms * 10_230,
        prn,
        kind,
        detail: detail.to_string(),
    })
    .collect()
}

fn event_table_header(palette: Palette) -> impl IntoElement {
    div()
        .h(px(30.))
        .px(px(14.))
        .flex()
        .items_center()
        .bg(palette.surface_alt)
        .border_b_1()
        .border_color(palette.border)
        .child(table_cell("TIME", 86.0, palette, true))
        .child(table_cell("PRN", 54.0, palette, true))
        .child(table_cell("EVENT", 112.0, palette, true))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(10.))
                .text_color(palette.muted)
                .child("DETAIL"),
        )
}

fn event_table_row(event: &GuiDiagnosticEvent, palette: Palette) -> impl IntoElement {
    let kind_color = event_kind_color(event, palette);
    div()
        .min_h(px(36.))
        .px(px(14.))
        .flex()
        .items_center()
        .border_b_1()
        .border_color(palette.border.opacity(0.55))
        .child(table_cell(
            format!("{:.3} s", event.now_ms as f64 / 1_000.0),
            86.0,
            palette,
            true,
        ))
        .child(table_cell(
            event
                .prn
                .map(|prn| format!("G{prn:02}"))
                .unwrap_or_else(|| "--".to_string()),
            54.0,
            palette,
            event.prn.is_none(),
        ))
        .child(
            div()
                .w(px(112.))
                .flex_shrink_0()
                .text_size(px(9.))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(kind_color)
                .child(event_kind_label(event)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_size(px(10.))
                .text_color(palette.text)
                .child(event.detail.clone()),
        )
}

fn candidate_kind_label(kind: PvtEpochSolutionKind) -> &'static str {
    match kind {
        PvtEpochSolutionKind::Raw => "Raw WLS",
        PvtEpochSolutionKind::Smoothed => "Smoothed WLS",
        PvtEpochSolutionKind::Kalman => "Kalman",
        PvtEpochSolutionKind::Unconverged => "Unconverged",
    }
}

fn toggled_side_drawer(current: Option<SideDrawer>, target: SideDrawer) -> Option<SideDrawer> {
    (current != Some(target)).then_some(target)
}

fn candidate_disposition_label(disposition: PvtCandidateDisposition) -> &'static str {
    match disposition {
        PvtCandidateDisposition::Selected => "已采用",
        PvtCandidateDisposition::Available => "可用 · 较低优先级",
        PvtCandidateDisposition::NonConverged => "未收敛",
        PvtCandidateDisposition::QualityRejected => "质量门限拒绝",
        PvtCandidateDisposition::Unavailable => "不可用",
    }
}

fn candidate_disposition_color(disposition: PvtCandidateDisposition, palette: Palette) -> Hsla {
    match disposition {
        PvtCandidateDisposition::Selected => palette.accent,
        PvtCandidateDisposition::Available => palette.healthy,
        PvtCandidateDisposition::NonConverged | PvtCandidateDisposition::QualityRejected => {
            palette.warning
        }
        PvtCandidateDisposition::Unavailable => palette.muted,
    }
}

fn candidate_empty_message(disposition: PvtCandidateDisposition) -> &'static str {
    match disposition {
        PvtCandidateDisposition::NonConverged => "本 epoch 未产生收敛候选解。",
        PvtCandidateDisposition::QualityRejected => "候选解被质量门限拒绝，数值未进入最终输出。",
        PvtCandidateDisposition::Unavailable => "该处理配置或当前 epoch 未产生此候选解。",
        _ => "候选数值不可用。",
    }
}

fn comparison_group_title(title: &'static str, palette: Palette) -> impl IntoElement {
    div()
        .mt(px(9.))
        .mb(px(2.))
        .text_size(px(9.))
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(palette.muted)
        .child(title)
}

fn comparison_metric(
    label: &'static str,
    value: impl Into<SharedString>,
    palette: Palette,
) -> impl IntoElement {
    div()
        .min_h(px(24.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .text_size(px(10.))
        .child(div().text_color(palette.muted).child(label))
        .child(
            div()
                .text_right()
                .font_family(data_font_family())
                .text_color(palette.text)
                .child(value.into()),
        )
}

fn format_optional_metric(value: Option<f64>, unit: &str, digits: usize) -> String {
    value
        .filter(|value| value.is_finite())
        .map(|value| format!("{value:.digits$} {unit}"))
        .unwrap_or_else(|| "不可用".to_string())
}

fn format_prns(prns: &[u8]) -> String {
    if prns.is_empty() {
        return "无".to_string();
    }
    prns.iter()
        .map(|prn| format!("G{prn:02}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

pub(crate) fn pvt_position_is_displayable(pvt: &GuiPvtViewModel) -> bool {
    let quality = gnss_core::pvt::PositionQualityConfig::default();
    pvt.quality.converged
        && pvt.ecef_m.iter().all(|value| value.is_finite())
        && pvt.latitude_deg.is_finite()
        && pvt.longitude_deg.is_finite()
        && pvt.altitude_m.is_finite()
        && (quality.min_altitude_m..=quality.max_altitude_m).contains(&pvt.altitude_m)
}

fn velocity_plot(
    epochs: &[GuiPvtViewModel],
    selected_epoch: Option<usize>,
    palette: Palette,
) -> impl IntoElement {
    let points = epochs
        .iter()
        .enumerate()
        .filter_map(|(index, pvt)| {
            pvt.velocity
                .as_ref()
                .map(|velocity| (index as f64, velocity.speed_mps))
        })
        .collect::<Vec<_>>();
    line_plot(
        points,
        palette.healthy,
        palette,
        selected_epoch.map(|index| index as f64),
    )
}

fn altitude_plot(
    epochs: &[GuiPvtViewModel],
    selected_epoch: Option<usize>,
    palette: Palette,
) -> impl IntoElement {
    let points = epochs
        .iter()
        .enumerate()
        .filter(|(_, pvt)| pvt_position_is_displayable(pvt))
        .map(|(index, pvt)| (index as f64, pvt.altitude_m))
        .collect::<Vec<_>>();
    let averaged_points = moving_average_points(&points, 10);
    line_plot_with_secondary(
        points,
        palette.muted,
        averaged_points,
        palette.accent,
        palette,
        selected_epoch.map(|index| index as f64),
    )
}

fn moving_average_points(points: &[(f64, f64)], depth: usize) -> Vec<(f64, f64)> {
    points
        .windows(depth)
        .filter(|window| (window[depth - 1].0 - window[0].0 - (depth - 1) as f64).abs() < 1e-9)
        .map(|window| {
            (
                window[depth - 1].0,
                window.iter().map(|point| point.1).sum::<f64>() / depth as f64,
            )
        })
        .collect()
}

fn line_plot(
    points: Vec<(f64, f64)>,
    color: Hsla,
    palette: Palette,
    selected_x: Option<f64>,
) -> impl IntoElement {
    line_plot_with_secondary(points, color, Vec::new(), color, palette, selected_x)
}

fn line_plot_with_secondary(
    points: Vec<(f64, f64)>,
    color: Hsla,
    secondary_points: Vec<(f64, f64)>,
    secondary_color: Hsla,
    palette: Palette,
    selected_x: Option<f64>,
) -> impl IntoElement {
    canvas(
        move |_, _, _| {},
        move |plot_bounds, _, window, _| {
            for ratio in [0.0_f32, 0.5, 1.0] {
                let y = plot_bounds.origin.y + plot_bounds.size.height * ratio;
                window.paint_quad(quad(
                    bounds(
                        point(plot_bounds.origin.x, y),
                        size(plot_bounds.size.width, px(1.)),
                    ),
                    px(0.),
                    palette.border,
                    px(0.),
                    palette.border,
                    Default::default(),
                ));
            }
            if points.is_empty() {
                return;
            }
            let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let mut y_values = points.iter().map(|point| point.1).collect::<Vec<_>>();
            y_values.sort_by(f64::total_cmp);
            let low_index = ((y_values.len() - 1) as f64 * 0.05).round() as usize;
            let high_index = ((y_values.len() - 1) as f64 * 0.95).round() as usize;
            let min_y = y_values[low_index];
            let max_y = y_values[high_index];
            let span_x = (max_x - min_x).abs().max(1e-12);
            let span_y = (max_y - min_y).abs().max(1e-12);
            let build_path = |series: &[(f64, f64)], width| {
                let mut builder = PathBuilder::stroke(width);
                for (index, (x, y)) in series.iter().enumerate() {
                    let px_x = plot_bounds.origin.x
                        + plot_bounds.size.width * ((*x - min_x) / span_x) as f32;
                    let normalized_y = (y.clamp(min_y, max_y) - min_y) / span_y;
                    let px_y = plot_bounds.origin.y
                        + plot_bounds.size.height * (1.0 - normalized_y as f32);
                    if index == 0 {
                        builder.move_to(point(px_x, px_y));
                    } else {
                        builder.line_to(point(px_x, px_y));
                    }
                }
                builder.build().ok()
            };
            if let Some(path) = build_path(&points, px(1.0)) {
                window.paint_path(path, color);
            }
            if let Some(path) = build_path(&secondary_points, px(2.0)) {
                window.paint_path(path, secondary_color);
            }
            if let Some((x, y)) = selected_x.and_then(|selected_x| {
                points
                    .iter()
                    .find(|(x, _)| (*x - selected_x).abs() < f64::EPSILON)
            }) {
                let px_x =
                    plot_bounds.origin.x + plot_bounds.size.width * ((*x - min_x) / span_x) as f32;
                let normalized_y = (y.clamp(min_y, max_y) - min_y) / span_y;
                let px_y =
                    plot_bounds.origin.y + plot_bounds.size.height * (1.0 - normalized_y as f32);
                window.paint_quad(quad(
                    bounds(point(px_x - px(4.), px_y - px(4.)), size(px(8.), px(8.))),
                    px(4.),
                    palette.accent,
                    px(1.),
                    palette.surface,
                    Default::default(),
                ));
            }
        },
    )
    .size_full()
}

#[cfg(test)]
mod settings_editor_tests {
    use super::{parse_fixed_prns, parse_initial_lla, setting_text_value};
    use crate::GuiProcessingSettings;

    #[test]
    fn settings_editor_round_trips_structured_text_fields() {
        let settings = GuiProcessingSettings {
            sample_rate_hz: 4_092_000.0,
            fixed_prns: Some(vec![1, 11, 32]),
            initial_position_lla_deg_m: Some([41.25, 1.98, 72.5]),
            ..Default::default()
        };

        assert_eq!(setting_text_value(&settings, "sample_rate_hz"), "4.092000");
        assert_eq!(setting_text_value(&settings, "fixed_prns"), "1,11,32");
        assert_eq!(parse_fixed_prns("1, 11; 32"), Ok(Some(vec![1, 11, 32])));
        assert_eq!(
            parse_initial_lla("41.25, 1.98, 72.5"),
            Ok(Some([41.25, 1.98, 72.5]))
        );
        assert!(parse_fixed_prns("1,abc").is_err());
        assert!(parse_initial_lla("41.25,1.98").is_err());
    }
}

#[cfg(test)]
mod position_quality_tests {
    use super::{moving_average_points, pvt_position_is_displayable};

    #[test]
    fn legacy_implausible_altitude_is_excluded_from_position_plots() {
        let mut valid = crate::sample_pvt_view_model();
        valid.altitude_m = 70.0;
        let mut implausible = valid.clone();
        implausible.altitude_m = 1_123_486.0;
        let mut stratospheric = valid.clone();
        stratospheric.altitude_m = 90_225.0;

        assert!(pvt_position_is_displayable(&valid));
        assert!(!pvt_position_is_displayable(&implausible));
        assert!(!pvt_position_is_displayable(&stratospheric));
    }

    #[test]
    fn altitude_moving_average_does_not_bridge_invalid_epoch_gaps() {
        let points = (0..12)
            .filter(|index| *index != 5)
            .map(|index| (index as f64, index as f64))
            .collect::<Vec<_>>();

        assert_eq!(moving_average_points(&points, 3).first(), Some(&(2.0, 1.0)));
        assert!(moving_average_points(&points, 3)
            .iter()
            .all(|(epoch, _)| *epoch != 6.0 && *epoch != 7.0));
    }
}

fn epoch_index_for_percent(count: usize, percent: f32) -> Option<usize> {
    (count > 0).then(|| ((percent.clamp(0.0, 100.0) / 100.0) * (count - 1) as f32).round() as usize)
}

/// Whole-percent label for a stored progress fraction (`0.0..=1.0`), used by
/// the workspace header, run rows and the status strip so every surface
/// reports the same number.
fn processing_percent_label(progress: f64) -> String {
    format!("{:.0}%", progress.clamp(0.0, 1.0) * 100.0)
}

fn ordered_queue(history: &[ProcessingRun], order: &[Uuid]) -> VecDeque<ProcessingRun> {
    let mut queued = history
        .iter()
        .filter(|run| run.status == RunStatus::Queued)
        .cloned()
        .collect::<Vec<_>>();
    queued.sort_by_key(|run| {
        order
            .iter()
            .position(|id| *id == run.id)
            .unwrap_or(usize::MAX)
    });
    queued.into()
}

fn move_queue_item<T>(queue: &mut VecDeque<T>, index: usize, direction: i32) -> bool {
    let target = if direction < 0 {
        index.checked_sub(1)
    } else if direction > 0 && index + 1 < queue.len() {
        Some(index + 1)
    } else {
        None
    };
    let Some(target) = target else {
        return false;
    };
    queue.swap(index, target);
    true
}

fn can_enqueue_run(
    _has_active_run: bool,
    has_setting_parse_errors: bool,
    has_settings_issues: bool,
) -> bool {
    !has_setting_parse_errors && !has_settings_issues
}

fn poll_decision(worker_connected: bool, drained_events: usize) -> (bool, bool) {
    (drained_events > 0, worker_connected)
}

#[derive(Clone, Copy)]
struct PersistenceProgress<'a> {
    run_id: Uuid,
    status: RunStatus,
    processed_samples: u64,
    total_samples: u64,
    pvt_epoch_count: usize,
    message: &'a str,
}

fn persist_progress(
    writer: Option<&RunPersistenceWriter>,
    library: Option<&RunLibrary>,
    progress: PersistenceProgress<'_>,
) {
    let queued = writer.is_some_and(|writer| {
        writer
            .update_progress(
                progress.run_id,
                progress.status,
                progress.processed_samples,
                progress.total_samples,
                progress.pvt_epoch_count,
                progress.message,
            )
            .is_ok()
    });
    if !queued {
        if let Some(library) = library {
            let _ = library.update_progress(
                progress.run_id,
                progress.status,
                progress.processed_samples,
                progress.total_samples,
                progress.pvt_epoch_count,
                progress.message,
            );
        }
    }
}

fn persist_epoch(
    writer: Option<&RunPersistenceWriter>,
    library: Option<&RunLibrary>,
    run_id: Uuid,
    epoch_ordinal: usize,
    epoch: &GuiPvtViewModel,
    channels: &[GuiChannelStatus],
) {
    let queued = writer.is_some_and(|writer| {
        writer
            .append_epoch(run_id, epoch_ordinal, epoch.clone(), channels.to_vec())
            .is_ok()
    });
    if !queued {
        if let Some(library) = library {
            let _ = library.append_epoch(run_id, epoch_ordinal, epoch, channels);
        }
    }
}

fn persist_event(
    writer: Option<&RunPersistenceWriter>,
    library: Option<&RunLibrary>,
    run_id: Uuid,
    event_ordinal: usize,
    event: &GuiDiagnosticEvent,
) {
    let queued = writer.is_some_and(|writer| {
        writer
            .append_event(run_id, event_ordinal, event.clone())
            .is_ok()
    });
    if !queued {
        if let Some(library) = library {
            let _ = library.append_event(run_id, event_ordinal, event);
        }
    }
}

fn finalize_persistence(
    writer: Option<&RunPersistenceWriter>,
    library: Option<&RunLibrary>,
    progress: PersistenceProgress<'_>,
) {
    let finalized = writer.is_some_and(|writer| {
        writer
            .finalize(
                progress.run_id,
                progress.status,
                progress.processed_samples,
                progress.total_samples,
                progress.pvt_epoch_count,
                progress.message,
            )
            .is_ok()
    });
    if !finalized {
        if let Some(library) = library {
            let _ = library.update_progress(
                progress.run_id,
                progress.status,
                progress.processed_samples,
                progress.total_samples,
                progress.pvt_epoch_count,
                progress.message,
            );
        }
    }
}

const DOCK_HISTORY_LIMIT: usize = 80;

struct DockHistoryWindow<'a> {
    rows: Vec<&'a ProcessingRun>,
    matching: usize,
}

fn dock_history_window<'a>(
    history: &'a [ProcessingRun],
    active_id: Option<Uuid>,
    query: &str,
    filter: HistoryFilter,
) -> DockHistoryWindow<'a> {
    let mut window = DockHistoryWindow {
        rows: Vec::with_capacity(history.len().min(DOCK_HISTORY_LIMIT)),
        matching: 0,
    };
    for run in history {
        if run.status == RunStatus::Queued
            || Some(run.id) == active_id
            || !history_run_matches(run, query, filter)
        {
            continue;
        }
        window.matching += 1;
        if window.rows.len() < DOCK_HISTORY_LIMIT {
            window.rows.push(run);
        }
    }
    window
}

fn history_run_matches(run: &ProcessingRun, query: &str, filter: HistoryFilter) -> bool {
    let status_matches = match filter {
        HistoryFilter::All => true,
        HistoryFilter::Completed => run.status == RunStatus::Completed,
        HistoryFilter::NeedsAttention => matches!(
            run.status,
            RunStatus::Failed | RunStatus::Cancelled | RunStatus::Interrupted
        ),
    };
    if !status_matches {
        return false;
    }
    let query = query.trim().to_lowercase();
    query.is_empty()
        || run
            .source_path
            .to_string_lossy()
            .to_lowercase()
            .contains(&query)
        || format!("{:?}", run.status).to_lowercase().contains(&query)
}

#[cfg(test)]
mod history_queue_tests {
    use super::*;
    use chrono::Utc;

    #[gpui_kit::test]
    fn context_menu_selects_target_and_keeps_narrow_library_open(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(gpui_kit::component::init);
        let library = RunLibrary::open(":memory:").unwrap();
        let target = library
            .create_run(&GuiProcessingSettings::default())
            .unwrap();
        library
            .update_progress(target.id, RunStatus::Completed, 1, 1, 0, "fixture")
            .unwrap();
        let (app, visual) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        visual.update(|window, cx| {
            let builder = app.update(cx, |app, cx| {
                app.selected_run_id = None;
                app.compact_library_expanded = true;
                app.history_context_menu(target.id, cx)
            });
            let _menu = PopupMenu::build(window, cx, builder);
            app.update(cx, |app, _| {
                assert_eq!(app.selected_run_id, Some(target.id));
                assert!(app.compact_library_expanded);
                assert_eq!(app.history.len(), 1);
            });
        });
    }

    #[gpui_kit::test]
    fn history_delete_requires_confirmation_and_keeps_original_target(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(gpui_kit::component::init);
        let library = RunLibrary::open(":memory:").unwrap();
        let first = library
            .create_run(&GuiProcessingSettings::default())
            .unwrap();
        let second = library
            .create_run(&GuiProcessingSettings::default())
            .unwrap();
        for id in [first.id, second.id] {
            library
                .update_progress(id, RunStatus::Completed, 1, 1, 0, "fixture")
                .unwrap();
        }
        let (app, visual) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.interface_language = InterfaceLanguage::English;
                app.request_delete_historical_run(first.id, window, cx);
                assert_eq!(app.history.len(), 2, "request cannot delete");
            })
        });
        visual.simulate_prompt_answer("Cancel");
        visual.run_until_parked();
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.history.len(), 2, "cancel keeps both records");
                app.request_delete_historical_run(first.id, window, cx);
                app.selected_run_id = Some(second.id);
                app.history.reverse();
            })
        });
        visual.simulate_prompt_answer("Delete run");
        visual.run_until_parked();
        app.update(visual, |app, _| {
            assert_eq!(app.history.len(), 1);
            assert_eq!(
                app.history[0].id, second.id,
                "confirmation must use captured ID"
            );
        });
    }

    #[gpui_kit::test]
    fn cancelling_queued_copy_does_not_stop_real_worker(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        struct TestIq(PathBuf);
        impl Drop for TestIq {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let source = TestIq(
            std::env::temp_dir().join(format!("gnsscope-queue-worker-{}.iq", Uuid::new_v4())),
        );
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source.0)
            .unwrap()
            .set_len(8_000)
            .unwrap();
        // Deliberately undersampled zero IQ: scheduling evidence, not RF/PVT proof.
        // 2,000 progress epochs exceed the production channel capacity, preventing
        // the producer from finishing before we consume its first progress event.
        let settings = GuiProcessingSettings {
            iq_file_path: Some(source.0.clone()),
            sample_rate_hz: 1_000.0,
            iq_format: crate::GuiIqFormat::Int8Interleaved,
            chunk_ms: 2,
            pvt_output_rate_ms: 2,
            fixed_prns: Some(vec![1]),
            ..Default::default()
        };
        let library = RunLibrary::open(":memory:").unwrap();
        let original = library.create_run(&settings).unwrap();
        library
            .append_epoch(original.id, 0, &crate::sample_pvt_view_model(), &[])
            .unwrap();
        library
            .update_progress(
                original.id,
                RunStatus::Completed,
                4_000,
                4_000,
                1,
                "fixture",
            )
            .unwrap();
        let original_id = original.id;
        let (app, cx) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        app.update(cx, |app, cx| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                app.settings = settings;
                app.enqueue(cx);
                let active_id = app.active_run.as_ref().expect("A started").id;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                let receive = |app: &mut GnsscopeApp| {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    let event = app
                        .worker_receiver
                        .as_ref()
                        .unwrap()
                        .recv_timeout(remaining)
                        .expect("worker event before deadline");
                    assert!(
                        !matches!(
                            event,
                            WorkerEvent::Failed(_) | WorkerEvent::Cancelled { .. }
                        ),
                        "A must not fail or be cancelled: {event:?}"
                    );
                    app.apply_worker_event(event);
                };
                while app.active_run.as_ref().unwrap().processed_samples == 0 {
                    receive(app);
                }
                let before = app.active_run.as_ref().unwrap().processed_samples;
                assert!(!app.worker.as_ref().unwrap().is_finished());
                app.enqueue(cx);
                let queued_id = app.queue.front().expect("B queued").id;
                assert_ne!(queued_id, active_id);
                app.cancel_queued(queued_id);
                assert!(app.queue.is_empty());
                assert_eq!(app.active_run.as_ref().unwrap().id, active_id);
                assert!(!app.worker.as_ref().unwrap().is_finished());
                let mut advanced = false;
                while app.active_run.is_some() {
                    receive(app);
                    if let Some(active) = &app.active_run {
                        assert_eq!(active.id, active_id);
                        advanced |= active.processed_samples > before;
                    }
                }
                assert!(advanced, "A advances after B cancellation");
                let library = app.library.as_ref().unwrap();
                let runs = library.list_runs().unwrap();
                let completed = runs.iter().find(|run| run.id == active_id).unwrap();
                assert_eq!(completed.status, RunStatus::Completed);
                assert_eq!(completed.processed_samples, 4_000);
                assert_eq!(completed.total_samples, 4_000);
                assert_eq!(completed.pvt_epoch_count, 0);
                let cancelled = runs.iter().find(|run| run.id == queued_id).unwrap();
                assert_eq!(cancelled.status, RunStatus::Cancelled);
                assert_eq!(cancelled.processed_samples, 0);
                assert_eq!(library.epochs(original_id).unwrap().len(), 1);
                assert_eq!(
                    library.preference("queue_order").unwrap().as_deref(),
                    Some("[]")
                );
                app.delete_historical_run(queued_id);
                let runs = app.library.as_ref().unwrap().list_runs().unwrap();
                assert_eq!(runs.len(), 2);
                assert!(runs
                    .iter()
                    .any(|run| run.id == original_id && run.status == RunStatus::Completed));
                assert_eq!(std::fs::metadata(&source.0).unwrap().len(), 8_000);
                assert_eq!(std::fs::read(&source.0).unwrap(), vec![0; 8_000]);
                assert_eq!(
                    app.library
                        .as_ref()
                        .unwrap()
                        .epochs(original_id)
                        .unwrap()
                        .len(),
                    1
                );
            }));
            // Disconnect before joining on assertion failure: a bounded sender may
            // otherwise remain blocked while ActiveWorker::drop waits for it.
            app.worker_receiver.take();
            app.worker.take();
            if let Err(error) = result {
                std::panic::resume_unwind(error);
            }
        });
    }

    #[gpui_kit::test]
    fn enqueue_and_cancel_preserve_active_and_completed_history(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        let library = RunLibrary::open(":memory:").unwrap();
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("isolated-workflow.dat")),
            ..Default::default()
        };
        let original = library.create_run(&settings).unwrap();
        library
            .append_epoch(original.id, 0, &crate::sample_pvt_view_model(), &[])
            .unwrap();
        library
            .update_progress(original.id, RunStatus::Completed, 100, 100, 1, "fixture")
            .unwrap();
        let original_id = original.id;
        let (app, cx) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        app.update(cx, |app, cx| {
            let active = run("active.dat", RunStatus::Processing);
            let active_id = active.id;
            app.active_run = Some(active);
            app.live_run_id = Some(active_id);
            app.epochs = vec![crate::sample_pvt_view_model(); 2];
            app.settings = settings;
            app.selected_run_id = Some(original_id);
            app.historical_epochs = vec![crate::sample_pvt_view_model()];
            app.historical_channels = vec![Vec::new()];
            app.historical_diagnostic_events = preview_diagnostic_events();
            app.selected_epoch = Some(0);
            app.follow_live = true;

            app.enqueue(cx);
            let queued_id = app.queue.front().expect("new queued run").id;
            assert_ne!(queued_id, original_id);
            assert_eq!(app.selected_run_id, Some(queued_id));
            assert!(app.display_epochs().is_empty());
            assert!(app.historical_channels.is_empty());
            assert!(app.historical_diagnostic_events.is_empty());
            assert_eq!(app.selected_epoch, None);
            assert!(!app.follow_live);
            assert_eq!(app.active_run.as_ref().unwrap().id, active_id);
            assert_eq!(app.live_run_id, Some(active_id));
            assert_eq!(app.epochs.len(), 2);

            app.cancel_queued(queued_id);
            assert!(app.queue.is_empty());
            assert_eq!(app.selected_run_id, None);
            assert_eq!(app.active_run.as_ref().unwrap().id, active_id);
            assert_eq!(app.epochs.len(), 2);
            let library = app.library.as_ref().unwrap();
            let persisted = library.list_runs().unwrap();
            assert_eq!(
                persisted
                    .iter()
                    .find(|run| run.id == queued_id)
                    .unwrap()
                    .status,
                RunStatus::Cancelled
            );
            assert_eq!(
                persisted
                    .iter()
                    .find(|run| run.id == original_id)
                    .unwrap()
                    .status,
                RunStatus::Completed
            );
            assert_eq!(library.epochs(original_id).unwrap().len(), 1);
            assert_eq!(
                library.preference("queue_order").unwrap().as_deref(),
                Some("[]")
            );

            // Exercise the app command too, not just the storage API.
            app.delete_historical_run(queued_id);
            let library = app.library.as_ref().unwrap();
            assert_eq!(library.list_runs().unwrap().len(), 1);
            assert_eq!(library.epochs(original_id).unwrap().len(), 1);
            assert_eq!(app.active_run.as_ref().unwrap().id, active_id);
            assert_eq!(app.epochs.len(), 2);
        });
    }

    fn run(name: &str, status: RunStatus) -> ProcessingRun {
        ProcessingRun {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            source_path: PathBuf::from(name),
            status,
            progress: 0.0,
            processed_samples: 0,
            total_samples: 0,
            pvt_epoch_count: 0,
            message: String::new(),
            settings: GuiProcessingSettings::default(),
        }
    }

    #[test]
    fn dock_history_counts_matches_without_rendering_unbounded_rows() {
        let history = (0..101)
            .map(|index| run(&format!("capture-{index:03}.dat"), RunStatus::Completed))
            .collect::<Vec<_>>();
        let window = dock_history_window(&history, None, "", HistoryFilter::All);
        assert_eq!(window.matching, 101);
        assert_eq!(window.rows.len(), DOCK_HISTORY_LIMIT);
        assert_eq!(window.rows[0].id, history[0].id);
        assert_eq!(window.rows[79].id, history[79].id);
        // Searching can reach a record beyond the initial rendering limit.
        let older = dock_history_window(&history, None, "100", HistoryFilter::Completed);
        assert_eq!(older.matching, 1);
        assert_eq!(older.rows[0].id, history[100].id);
    }

    #[test]
    fn dock_history_excludes_active_queue_and_nonmatching_statuses() {
        let history = vec![
            run("active.dat", RunStatus::Processing),
            run("queued.dat", RunStatus::Queued),
            run("failed.dat", RunStatus::Failed),
            run("done.dat", RunStatus::Completed),
        ];
        let active_id = Some(history[0].id);
        let all = dock_history_window(&history, active_id, "", HistoryFilter::All);
        assert_eq!(all.matching, 2);
        let attention = dock_history_window(&history, active_id, "", HistoryFilter::NeedsAttention);
        assert_eq!(attention.matching, 1);
        assert_eq!(attention.rows[0].id, history[2].id);
        let empty = dock_history_window(&history, active_id, "missing", HistoryFilter::All);
        assert_eq!(empty.matching, 0);
        assert!(empty.rows.is_empty());
    }

    #[test]
    fn queue_order_and_move_are_deterministic() {
        let first = run("first.dat", RunStatus::Queued);
        let second = run("second.dat", RunStatus::Queued);
        let history = vec![first.clone(), second.clone()];
        let mut queue = ordered_queue(&history, &[second.id, first.id]);

        assert_eq!(queue[0].id, second.id);
        assert!(move_queue_item(&mut queue, 1, -1));
        assert_eq!(queue[0].id, first.id);
        assert!(!move_queue_item(&mut queue, 0, -1));
    }

    #[test]
    fn history_search_combines_path_and_status_filter() {
        let completed = run("captures/CTTC_SPAIN.dat", RunStatus::Completed);
        let failed = run("captures/field.dat", RunStatus::Failed);

        assert!(history_run_matches(
            &completed,
            "cttc",
            HistoryFilter::Completed
        ));
        assert!(!history_run_matches(
            &failed,
            "field",
            HistoryFilter::Completed
        ));
        assert!(history_run_matches(
            &failed,
            "field",
            HistoryFilter::NeedsAttention
        ));
    }

    #[test]
    fn valid_run_can_be_enqueued_while_another_run_is_active() {
        assert!(can_enqueue_run(true, false, false));
        assert!(!can_enqueue_run(true, true, false));
        assert!(!can_enqueue_run(true, false, true));
    }

    #[test]
    fn worker_poll_only_redraws_for_events_and_stops_when_disconnected() {
        assert_eq!(poll_decision(true, 0), (false, true));
        assert_eq!(poll_decision(true, 2), (true, true));
        assert_eq!(poll_decision(false, 0), (false, false));
    }

    // -----------------------------------------------------------------------
    // Issue 17 acceptance tests (items 1-5)
    // -----------------------------------------------------------------------

    fn channel(prn: u8, tracking_locked: bool, nav_locked: bool) -> GuiChannelStatus {
        GuiChannelStatus {
            prn,
            acquired: true,
            tracking_locked,
            nav_locked,
            cn0_db_hz: 42.0,
            doppler_hz: -1200.0,
            residual_m: Some(1.5),
        }
    }

    /// Item 3: browsing a historical run must surface that run's own epoch and
    /// channel view, never the live worker's counts (issue 17 review finding).
    #[gpui_kit::test]
    fn selected_run_context_is_isolated_from_live_processing(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        let (app, visual) =
            cx.add_window_view(|window, cx| GnsscopeApp::with_library(window, cx, None, false));
        visual.update(|_, cx| {
            app.update(cx, |app, _| {
                let live_id = Uuid::new_v4();
                let history_id = Uuid::new_v4();
                // A live run is processing with rich channel/PVT data.
                app.live_run_id = Some(live_id);
                app.active_run = Some(run("live.dat", RunStatus::Processing));
                app.channels = vec![channel(1, true, true), channel(2, true, false)];
                app.epochs = vec![crate::sample_pvt_view_model()];
                app.epoch_channels = vec![vec![channel(1, true, true)]];
                // A selected historical run has its own, distinct view.
                app.historical_epochs = vec![crate::sample_pvt_view_model()];
                app.historical_channels = vec![vec![
                    channel(5, true, true),
                    channel(6, true, true),
                    channel(7, true, true),
                ]];
                app.selected_run_id = Some(history_id);
                app.selected_epoch = Some(0);

                // display_* resolve to the SELECTED (historical) run.
                assert_eq!(app.display_epochs().len(), 1);
                let channels = app.display_channels();
                assert_eq!(channels.len(), 3, "historical channels, not live");
                assert_eq!(channels[0].prn, 5);
                assert!(
                    channels.iter().all(|channel| channel.nav_locked),
                    "the historical epoch is fully locked, unlike the live set"
                );
            });
        });
    }

    /// Item 1: the palette state machine opens, filters by the command-input
    /// query, and maps a filtered index back to the right command/run.
    #[gpui_kit::test]
    fn command_palette_opens_filters_and_selects(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        let library = RunLibrary::open(":memory:").unwrap();
        let completed = library
            .create_run(&GuiProcessingSettings::default())
            .unwrap();
        library
            .update_progress(completed.id, RunStatus::Completed, 1, 1, 0, "fixture")
            .unwrap();
        let (app, visual) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                // Opens closed → open, focusing the command field.
                assert!(!app.command_palette_open);
                app.open_command_palette(window, cx);
                assert!(app.command_palette_open);
                assert_eq!(app.command_highlight, Some(0));

                // Every global command plus the completed run is present.
                let all = app.palette_results();
                assert!(
                    all.iter()
                        .any(|(_, command)| *command == PaletteCommand::OpenRunLibrary),
                    "global commands are offered"
                );
                assert!(
                    all.iter()
                        .any(|(_, command)| matches!(command, PaletteCommand::SelectRun(id) if *id == completed.id)),
                    "the persisted run is offered"
                );

                // Typing a query that only the run title matches leaves it as
                // the sole filtered target (the shared substring rule).
                app.command_input
                    .update(cx, |input, cx| input.set_value("no-such-entry", window, cx));
                assert!(
                    app.palette_filtered_commands(cx).is_empty(),
                    "an unmatched query yields no executable target"
                );
                app.command_input
                    .update(cx, |input, cx| input.set_value("运行库", window, cx));
                let filtered = app.palette_filtered_commands(cx);
                assert_eq!(filtered, vec![PaletteCommand::OpenRunLibrary]);
                app.command_highlight = Some(0);

                // Selecting the highlighted entry navigates and closes.
                app.execute_palette_command(PaletteCommand::OpenRunLibrary, window, cx);
                assert!(matches!(app.page, AppPage::RunLibrary));
                app.open_command_palette(window, cx);
                app.close_command_palette(window, cx);
                assert!(!app.command_palette_open);
                assert_eq!(app.command_highlight, None);
            });
        });
    }

    /// Item 5: a persisted run whose source file is gone reports 源不可用; a
    /// completed run with a real file reports 已完成 with a healthy color.
    #[gpui_kit::test]
    fn runs_row_view_flags_missing_source_and_keeps_completed_honest(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(gpui_kit::component::init);
        let (app, visual) =
            cx.add_window_view(|window, cx| GnsscopeApp::with_library(window, cx, None, false));
        visual.update(|_, cx| {
            app.update(cx, |app, _| {
                app.interface_language = InterfaceLanguage::SimplifiedChinese;
                let palette = Palette::light();

                let missing = run("definitely/missing/capture.iq", RunStatus::Completed);
                let view = app.runs_row_view(&missing, palette);
                assert_eq!(view.status, "源不可用");
                assert_eq!(view.status_color, palette.warning);

                // A queued run is never flagged 源不可用 (it has not started).
                let queued = run("definitely/missing/queued.iq", RunStatus::Queued);
                let queued_view = app.runs_row_view(&queued, palette);
                assert_eq!(queued_view.status, "排队");

                // A completed run whose source exists keeps the honest
                // 已完成 label and healthy color, and carries its real size.
                let existing =
                    std::env::temp_dir().join(format!("gnsscope-row-{}.iq", Uuid::new_v4()));
                std::fs::write(&existing, [0u8; 2048]).unwrap();
                let mut done = run("done.dat", RunStatus::Completed);
                done.source_path = existing.clone();
                done.total_samples = 4_000_000;
                let done_view = app.runs_row_view(&done, palette);
                assert_eq!(done_view.status, "已完成");
                assert_eq!(done_view.status_color, palette.healthy);
                assert_eq!(done_view.samples_total, Some(4_000_000));
                assert_eq!(done_view.size_bytes, Some(2048));
                assert_eq!(
                    done_view.source_path.as_deref(),
                    Some(existing.to_string_lossy().as_ref())
                );
                let _ = std::fs::remove_file(&existing);
            });
        });
    }
}

#[cfg(test)]
mod timeline_interaction_tests {
    use super::*;

    /// The transparent seek slider over the timeline track must be the shared
    /// epoch cursor: a press jumps to that position and dragging moves the
    /// cursor continuously with the pointer.
    #[gpui_kit::test]
    fn timeline_seek_click_and_drag_select_epochs(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        let library = RunLibrary::open(":memory:").unwrap();
        let run = library
            .create_run(&GuiProcessingSettings::default())
            .unwrap();
        library
            .update_progress(run.id, RunStatus::Completed, 1, 1, 0, "fixture")
            .unwrap();
        let (app, visual) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        visual.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.selected_run_id = Some(run.id);
                let (epochs, channels) = preview_epoch_history();
                app.historical_epochs = epochs;
                app.historical_channels = channels;
                app.selected_epoch = Some(0);
                cx.notify();
            });
        });
        visual.run_until_parked();
        let (bounds, count) = visual.update(|_, cx| {
            let app = app.read(cx);
            (app.timeline.read(cx).bounds(), app.display_epochs().len())
        });
        assert!(count > 1, "fixture must expose a multi-epoch timeline");
        assert!(
            bounds.size.width > px(0.) && bounds.size.height > px(0.),
            "seek slider must have layout bounds on the timeline track: {bounds:?}"
        );
        let y = bounds.top() + bounds.size.height / 2.;
        let at = |fraction| bounds.left() + bounds.size.width * fraction;

        // A press on the track jumps the shared cursor.
        visual.simulate_mouse_down(
            point(at(0.25), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        visual.simulate_mouse_up(
            point(at(0.25), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        let clicked = visual.update(|_, cx| app.read(cx).selected_epoch);
        assert_eq!(
            clicked,
            epoch_index_for_percent(count, 25.0),
            "pressing the seek slider must select that point of the record"
        );

        // Dragging from 25% to 75% must track the pointer. GPUI starts the
        // drag on the first move past the threshold and delivers drag moves
        // from the following events on, so a real drag needs several moves.
        visual.simulate_mouse_down(
            point(at(0.25), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        visual.run_until_parked();
        for fraction in [0.35, 0.5, 0.6, 0.7, 0.75] {
            visual.simulate_mouse_move(
                point(at(fraction), y),
                Some(gpui_kit::MouseButton::Left),
                gpui_kit::Modifiers::default(),
            );
            visual.run_until_parked();
            if fraction >= 0.5 {
                assert_eq!(
                    visual.update(|_, cx| app.read(cx).selected_epoch),
                    epoch_index_for_percent(count, fraction * 100.0),
                    "dragging must continue across rendered frames at {fraction}"
                );
            }
        }
        visual.simulate_mouse_up(
            point(at(0.75), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        let dragged = visual.update(|_, cx| app.read(cx).selected_epoch);
        assert_eq!(
            dragged,
            epoch_index_for_percent(count, 75.0),
            "dragging the seek slider must move the shared epoch cursor with the pointer"
        );
    }

    /// The playhead cap and its callout are the visible grab handle of the
    /// timeline: a press near the top of the track must seek like a press on
    /// the baseline, not fall through a thin interaction strip.
    #[gpui_kit::test]
    fn timeline_seek_grabs_the_playhead_region(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        let library = RunLibrary::open(":memory:").unwrap();
        let run = library
            .create_run(&GuiProcessingSettings::default())
            .unwrap();
        library
            .update_progress(run.id, RunStatus::Completed, 1, 1, 0, "fixture")
            .unwrap();
        let (app, visual) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        visual.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.selected_run_id = Some(run.id);
                let (epochs, channels) = preview_epoch_history();
                app.historical_epochs = epochs;
                app.historical_channels = channels;
                app.selected_epoch = Some(0);
                cx.notify();
            });
        });
        visual.run_until_parked();
        let (bounds, count) = visual.update(|_, cx| {
            let app = app.read(cx);
            (app.timeline.read(cx).bounds(), app.display_epochs().len())
        });
        // The playhead is near the top of the painted 85px track. Derive
        // its position from the track center, not the slider's hitbox height:
        // the old styled slider exposed only a 6px strip in the middle.
        let y = bounds.center().y - px(metrics::TIMELINE_TRACK_HEIGHT / 2.0) + px(3.);
        let at = |fraction| point(bounds.left() + bounds.size.width * fraction, y);
        let press = at(0.5);
        visual.simulate_mouse_down(
            press,
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        visual.simulate_mouse_up(
            press,
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        let clicked = visual.update(|_, cx| app.read(cx).selected_epoch);
        assert_eq!(
            clicked,
            epoch_index_for_percent(count, 50.0),
            "pressing the playhead region of the track must select that point of the record"
        );
        visual.run_until_parked();
        visual.simulate_mouse_down(
            at(0.5),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        for fraction in [0.6, 0.7, 0.75] {
            visual.simulate_mouse_move(
                at(fraction),
                Some(gpui_kit::MouseButton::Left),
                gpui_kit::Modifiers::default(),
            );
            visual.run_until_parked();
        }
        visual.simulate_mouse_up(
            at(0.75),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        assert_eq!(
            visual.update(|_, cx| app.read(cx).selected_epoch),
            epoch_index_for_percent(count, 75.0),
            "dragging the visible playhead must move the shared epoch cursor"
        );
    }

    /// At zoom the seek slider spans the visible window, so its track
    /// fractions must map over that window and the window must stay put
    /// while the pointer drags — otherwise the playhead freezes at the
    /// track middle and the timeline appears impossible to drag.
    #[gpui_kit::test]
    fn timeline_seek_tracks_the_pointer_when_zoomed(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::component::init);
        let library = RunLibrary::open(":memory:").unwrap();
        let run = library
            .create_run(&GuiProcessingSettings::default())
            .unwrap();
        library
            .update_progress(run.id, RunStatus::Completed, 1, 1, 0, "fixture")
            .unwrap();
        let (app, visual) = cx.add_window_view(move |window, cx| {
            GnsscopeApp::with_library(window, cx, Some(library), false)
        });
        visual.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.selected_run_id = Some(run.id);
                let (epochs, channels) = preview_epoch_history();
                app.historical_epochs = epochs;
                app.historical_channels = channels;
                app.selected_epoch = Some(0);
                app.adjust_timeline_zoom(true);
                cx.notify();
            });
        });
        visual.run_until_parked();
        let (bounds, count) = visual.update(|_, cx| {
            let app = app.read(cx);
            (app.timeline.read(cx).bounds(), app.display_epochs().len())
        });
        let y = bounds.top() + bounds.size.height / 2.;
        let at = |fraction| bounds.left() + bounds.size.width * fraction;

        // Zoom 2x on the record head shows the window [0, 0.5]; 75% of the
        // track is therefore 37.5% of the record.
        visual.simulate_mouse_down(
            point(at(0.75), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        let first = visual.update(|_, cx| app.read(cx).selected_epoch);
        assert_eq!(
            first,
            epoch_index_for_percent(count, 37.5),
            "a zoomed seek must map over the visible window, not the whole record"
        );

        // The window must not re-center under the pointer mid-drag: a second
        // press at 25% of the same window lands on 12.5% of the record.
        visual.simulate_mouse_up(
            point(at(0.75), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        visual.simulate_mouse_down(
            point(at(0.25), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        let second = visual.update(|_, cx| app.read(cx).selected_epoch);
        assert_eq!(
            second,
            epoch_index_for_percent(count, 12.5),
            "the visible window must stay put while the seek slider drives selection"
        );
        let cursor = visual.update(|_, cx| {
            let app = app.read(cx);
            let (start, span) = app.timeline_window();
            (app.timeline_cursor_fraction() - start) / span
        });
        assert!(
            (cursor - 0.25).abs() < 0.01,
            "the playhead must sit under the pointer fraction of the track, got {cursor}"
        );
        visual.run_until_parked();
        for fraction in [0.35, 0.5, 0.6, 0.7, 0.75] {
            visual.simulate_mouse_move(
                point(at(fraction), y),
                Some(gpui_kit::MouseButton::Left),
                gpui_kit::Modifiers::default(),
            );
            visual.run_until_parked();
        }
        visual.simulate_mouse_up(
            point(at(0.75), y),
            gpui_kit::MouseButton::Left,
            gpui_kit::Modifiers::default(),
        );
        assert_eq!(
            visual.update(|_, cx| app.read(cx).selected_epoch),
            epoch_index_for_percent(count, 37.5),
            "a zoomed drag must keep mapping to the same visible window"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        epoch_index_for_percent, epoch_jump_index, format_timeline_time, preview_diagnostic_events,
        preview_enu_position, preview_epoch_history, processing_percent_label, toggled_side_drawer,
        InterfaceLanguage, PlotFocus, SettingsCategory, SideDrawer, ThemePreference, WorkspaceView,
    };
    use crate::plots::cn0;
    use crate::plots::enu;

    #[test]
    fn processing_percent_label_treats_progress_as_a_fraction() {
        // The stored progress is 0.0..=1.0; the header once printed "1%" for
        // a 63% run because it clamped the fraction against 100.
        assert_eq!(processing_percent_label(0.63), "63%");
        assert_eq!(processing_percent_label(0.0), "0%");
        assert_eq!(processing_percent_label(1.0), "100%");
        assert_eq!(processing_percent_label(1.7), "100%");
        assert_eq!(processing_percent_label(-0.2), "0%");
    }

    #[test]
    fn precise_jump_parses_a_1_based_epoch_and_clamps_to_the_range() {
        assert_eq!(epoch_jump_index("1", 769), Some(0));
        assert_eq!(epoch_jump_index("120", 769), Some(119));
        assert_eq!(epoch_jump_index("1248", 769), Some(768));
        assert_eq!(epoch_jump_index(" 42 ", 769), Some(41));
        assert_eq!(epoch_jump_index("0", 769), None);
        assert_eq!(epoch_jump_index("", 769), None);
        assert_eq!(epoch_jump_index("abc", 769), None);
        assert_eq!(epoch_jump_index("5", 0), None);
    }

    #[test]
    fn timeline_percent_maps_to_shared_epoch_index() {
        assert_eq!(epoch_index_for_percent(0, 50.0), None);
        assert_eq!(epoch_index_for_percent(1, 0.0), Some(0));
        assert_eq!(epoch_index_for_percent(101, 0.0), Some(0));
        assert_eq!(epoch_index_for_percent(101, 50.0), Some(50));
        assert_eq!(epoch_index_for_percent(101, 100.0), Some(100));
    }

    #[test]
    fn timeline_labels_use_instrument_time_format() {
        assert_eq!(format_timeline_time(0), "00:00:00");
        assert_eq!(format_timeline_time(3_661_000), "01:01:01");
    }

    #[test]
    fn enu_extent_rounds_outward_to_instrument_ticks() {
        // Moved to `plots::enu` with the ENU track pane; kept here as a
        // regression anchor for the shared ladder.
        assert_eq!(enu::nice_enu_extent(0.4), 1.0);
        assert_eq!(enu::nice_enu_extent(1.3), 2.0);
        assert_eq!(enu::nice_enu_extent(3.1), 5.0);
        assert_eq!(enu::nice_enu_extent(8.2), 10.0);
        assert_eq!(enu::nice_enu_extent(42.0), 50.0);
    }

    #[test]
    fn side_drawers_are_mutually_exclusive() {
        assert_eq!(
            toggled_side_drawer(None, SideDrawer::Settings),
            Some(SideDrawer::Settings)
        );
        assert_eq!(
            toggled_side_drawer(Some(SideDrawer::Settings), SideDrawer::CandidateComparison),
            Some(SideDrawer::CandidateComparison)
        );
        assert_eq!(
            toggled_side_drawer(
                Some(SideDrawer::CandidateComparison),
                SideDrawer::CandidateComparison
            ),
            None
        );
    }

    #[test]
    fn workspace_views_route_domain_plots_to_their_expected_workbench() {
        assert_eq!(
            WorkspaceView::for_plot(PlotFocus::Sky),
            WorkspaceView::Overview
        );
        assert_eq!(
            WorkspaceView::for_plot(PlotFocus::Cn0),
            WorkspaceView::Channels
        );
        assert_eq!(
            WorkspaceView::for_plot(PlotFocus::Position),
            WorkspaceView::Position
        );
        assert_eq!(
            WorkspaceView::for_plot(PlotFocus::Velocity),
            WorkspaceView::Position
        );
        assert_eq!(
            WorkspaceView::for_plot(PlotFocus::Altitude),
            WorkspaceView::Position
        );
    }

    #[test]
    fn visual_preview_uses_bounded_deterministic_diagnostics() {
        let events = preview_diagnostic_events();

        assert_eq!(events.len(), 9);
        assert_eq!(events.first().and_then(|event| event.prn), Some(1));
        assert_eq!(events.last().and_then(|event| event.prn), Some(24));
        assert!(events
            .windows(2)
            .all(|pair| pair[0].now_ms < pair[1].now_ms));
    }

    #[test]
    fn visual_preview_provides_aligned_cn0_history() {
        let (epochs, channels) = preview_epoch_history();
        let series = cn0::build_cn0_series(
            &channels,
            epochs.len().checked_sub(1),
            Some(20),
            cn0::LANE_COUNT,
        );

        assert_eq!(epochs.len(), 769);
        assert_eq!(channels.len(), epochs.len());
        assert_eq!(series.first().map(|series| series.prn), Some(20));
        assert!(series
            .iter()
            .all(|series| series.samples.len() == epochs.len()));
        assert!(series.iter().any(|series| {
            let values = series.samples.iter().flatten().collect::<Vec<_>>();
            values
                .windows(2)
                .any(|window| (window[0] - window[1]).abs() > 1e-6)
        }));
    }

    #[test]
    fn visual_preview_trajectory_uses_concept_scale_and_selected_endpoint() {
        let points = (0..180)
            .map(|index| preview_enu_position(index, 180))
            .collect::<Vec<_>>();

        assert_eq!(points.first(), Some(&(0.0, 0.0)));
        assert_eq!(points.last(), Some(&(20.0, 1.0)));
        assert!(points.iter().any(|(east, _)| *east > 40.0));
        assert!(points.iter().any(|(_, north)| *north < -20.0));
    }

    #[test]
    fn settings_category_environment_values_are_stable() {
        assert_eq!(SettingsCategory::parse(None), SettingsCategory::Input);
        assert_eq!(
            SettingsCategory::parse(Some("tracking")),
            SettingsCategory::Tracking
        );
        assert_eq!(SettingsCategory::ALL.len(), 5);
        assert_eq!(
            SettingsCategory::ALL
                .map(|category| category.label(InterfaceLanguage::SimplifiedChinese)),
            ["输入", "捕获", "跟踪", "PVT", "性能"]
        );
        assert_eq!(
            SettingsCategory::ALL.map(|category| category.label(InterfaceLanguage::English)),
            ["Input", "Acquisition", "Tracking", "PVT", "Performance"]
        );
    }

    #[test]
    fn interface_preferences_use_stable_persistence_values() {
        assert_eq!(ThemePreference::parse(None).value(), "system");
        assert_eq!(ThemePreference::parse(Some("light")).value(), "light");
        assert_eq!(ThemePreference::parse(Some("dark")).value(), "dark");
        assert_eq!(InterfaceLanguage::parse(None).value(), "zh-CN");
        assert_eq!(InterfaceLanguage::parse(Some("en")).value(), "en");
    }
}
