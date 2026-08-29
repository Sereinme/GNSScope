use crate::{
    spawn_streaming_worker, ActiveWorker, GuiChannelStatus, GuiDiagnosticEvent,
    GuiDiagnosticEventKind, GuiProcessingSettings, GuiPvtViewModel, GuiSatelliteGeometry,
    GuiSettingsIssue, ProcessingRun, RunLibrary, RunPersistenceWriter, RunStatus, StoredEpoch,
    WorkerEvent, WORKER_EVENT_CAPACITY,
};
use gnss_core::pvt::{PvtCandidateDisposition, PvtEpochSolutionKind};
use gpui::prelude::*;
use gpui::{
    bounds, canvas, div, point, px, quad, rgb, size, AnyElement, Bounds, Context, Entity, Hsla,
    InteractiveElement, IntoElement, ParentElement, PathBuilder, PathPromptOptions, Render,
    SharedString, Styled, Timer, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    progress::Progress,
    scroll::ScrollableElement,
    slider::{Slider, SliderEvent, SliderState},
    switch::Switch,
    ActiveTheme, Disableable, IconName, Theme, ThemeMode, TitleBar,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

fn interface_font_family() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "SF Pro Text"
    }
    #[cfg(target_os = "windows")]
    {
        "Segoe UI Variable Text"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "Noto Sans"
    }
}

fn data_font_family() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "SF Mono"
    }
    #[cfg(target_os = "windows")]
    {
        "Cascadia Mono"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "monospace"
    }
}

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

    fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Completed => "已完成",
            Self::NeedsAttention => "需处理",
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

#[derive(Clone, Copy)]
struct Palette {
    background: Hsla,
    sidebar: Hsla,
    surface: Hsla,
    surface_alt: Hsla,
    border: Hsla,
    text: Hsla,
    muted: Hsla,
    accent: Hsla,
    healthy: Hsla,
    warning: Hsla,
    danger: Hsla,
    chart_series: [Hsla; 8],
}

impl Palette {
    fn light() -> Self {
        Self {
            background: color(0xf4f5f7),
            sidebar: color(0xf8f9fa),
            surface: color(0xffffff),
            surface_alt: color(0xf1f3f5),
            border: color(0xd8dce2),
            text: color(0x20242a),
            muted: color(0x66707c),
            accent: color(0x167a94),
            healthy: color(0x16845f),
            warning: color(0xa9630c),
            danger: color(0xb83f49),
            chart_series: [
                color(0x16845f),
                color(0x3578c8),
                color(0xa9630c),
                color(0x7446a6),
                color(0xc65f17),
                color(0x167a94),
                color(0x66707c),
                color(0xb83f49),
            ],
        }
    }

    fn dark() -> Self {
        Self {
            background: color(0x0e1013),
            sidebar: color(0x101318),
            surface: color(0x12151a),
            surface_alt: color(0x171b21),
            border: color(0x252b33),
            text: color(0xe6eaf0),
            muted: color(0x8b95a3),
            accent: color(0x63c5da),
            healthy: color(0x67c99a),
            warning: color(0xe0b15a),
            danger: color(0xe06c75),
            chart_series: [
                color(0x67c99a),
                color(0x7ab8ff),
                color(0xe0b15a),
                color(0xb792f4),
                color(0xed8a4b),
                color(0x63c5da),
                color(0xa7b0bd),
                color(0xe06c75),
            ],
        }
    }
}

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
    history_search: Entity<InputState>,
    setting_inputs: BTreeMap<&'static str, Entity<InputState>>,
    setting_parse_errors: BTreeMap<&'static str, &'static str>,
    history_filter: HistoryFilter,
    hovered_prn: Option<u8>,
    pinned_prn: Option<u8>,
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
    timeline_visible: bool,
    status_message: String,
}

impl GnsscopeApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = RunLibrary::open_default().ok();
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
        let timeline_visible = library
            .as_ref()
            .and_then(|library| library.preference("timeline_visible").ok().flatten())
            .is_none_or(|value| value != "false");
        let runs_sidebar_visible = library
            .as_ref()
            .and_then(|library| library.preference("runs_sidebar_visible").ok().flatten())
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
            let SliderEvent::Change(value) = event;
            this.select_epoch_percent(value.end());
            cx.notify();
        })
        .detach();
        let history_search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(match interface_language {
                    InterfaceLanguage::SimplifiedChinese => "搜索文件名或路径",
                    InterfaceLanguage::English => "Search file name or path",
                })
                .clean_on_escape()
        });
        cx.subscribe(&history_search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
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
            history_search,
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
            system_dark: cx.theme().mode.is_dark(),
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
            timeline_visible,
            status_message: "请选择 IQ 文件开始解算".to_string(),
        };
        if let Some(settings) = app.selected_run().map(|run| run.settings.clone()) {
            app.settings = settings;
            app.sync_setting_inputs(window, cx);
        }
        if std::env::var("GNSSCOPE_PREVIEW")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        {
            let (preview_epochs, preview_channels) = preview_epoch_history();
            app.history.clear();
            app.queue.clear();
            app.selected_run_id = None;
            app.live_run_id = None;
            app.historical_epochs = preview_epochs;
            app.historical_channels = preview_channels;
            app.historical_diagnostic_events = preview_diagnostic_events();
            app.selected_epoch = app.historical_epochs.len().checked_sub(1);
            app.runs_sidebar_visible = true;
            app.timeline_visible = true;
            app.status_message = "确定性界面预览".to_string();
        }
        if std::env::var_os("GNSSCOPE_AUTORUN").is_some() {
            app.enqueue(cx);
        }
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
                    InterfaceLanguage::SimplifiedChinese => "搜索文件名或路径",
                    InterfaceLanguage::English => "Search file name or path",
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

    fn schedule_poll(&mut self, cx: &mut Context<Self>) {
        if self.poll_scheduled || self.worker_receiver.is_none() {
            return;
        }
        self.poll_scheduled = true;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(50)).await;
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
                })
                .ok();
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
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn start_new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn cycle_iq_format(&mut self) {
        self.settings.iq_format = match self.settings.iq_format {
            crate::GuiIqFormat::Int8Interleaved => crate::GuiIqFormat::Int16Interleaved,
            crate::GuiIqFormat::Int16Interleaved => crate::GuiIqFormat::Int8Interleaved,
        };
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
        #[cfg(not(target_os = "windows"))]
        let result = std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(std::path::Path::new(".")))
            .spawn();
        self.status_message = if result.is_ok() {
            "已在文件管理器中定位源文件".to_string()
        } else {
            "无法打开文件管理器".to_string()
        };
    }

    fn delete_historical_run(&mut self, id: Uuid) {
        if self.active_run.as_ref().is_some_and(|run| run.id == id) {
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
        self.timeline.update(cx, |timeline, cx| {
            timeline.set_value(100.0, window, cx);
        });
    }

    fn select_epoch_percent(&mut self, percent: f32) {
        let count = self.display_epochs().len();
        self.selected_epoch = epoch_index_for_percent(count, percent);
        self.follow_live = false;
    }

    fn step_selected_epoch(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.display_epochs().len();
        if count == 0 {
            return;
        }
        let current = self.selected_epoch.unwrap_or(count - 1);
        let next =
            (current as i64 + delta as i64).clamp(0, count.saturating_sub(1) as i64) as usize;
        self.selected_epoch = Some(next);
        self.follow_live = false;
        let percent = if count <= 1 {
            100.0
        } else {
            next as f32 / (count - 1) as f32 * 100.0
        };
        self.timeline.update(cx, |timeline, cx| {
            timeline.set_value(percent, window, cx);
        });
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

    fn section_title(
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        palette: Palette,
    ) -> impl IntoElement {
        let title = title.into();
        let subtitle = subtitle.into();
        div()
            .flex()
            .flex_1()
            .items_baseline()
            .gap(px(7.))
            .min_w_0()
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(palette.text)
                    .child(title),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_size(px(10.))
                    .text_color(palette.muted)
                    .child(subtitle),
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
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
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
                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(palette.accent)
                            .child(div().size(px(6.)).rounded_full().bg(palette.accent))
                            .child(self.localized("正在处理", "Processing"))
                    }))
                    .children((self.page == AppPage::NewAnalysis).then(|| {
                        Button::new("title-select-file")
                            .ghost()
                            .text_color(palette.text)
                            .icon(IconName::FolderOpen)
                            .label(self.localized("选择 IQ", "Choose IQ"))
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
                                Button::new("return-live")
                                    .ghost()
                                    .text_color(palette.accent)
                                    .icon(IconName::Redo2)
                                    .tooltip(
                                        self.localized("返回最新 epoch", "Return to latest epoch"),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.return_to_live(window, cx);
                                        cx.notify();
                                    }))
                            },
                        ),
                    )
                    .children((self.page == AppPage::Workspace).then(|| {
                        Button::new("settings")
                            .ghost()
                            .text_color(palette.text)
                            .icon(IconName::Settings2)
                            .tooltip(
                                self.localized("接收机详细配置", "Receiver processing settings"),
                            )
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
                        Button::new("start-analysis")
                            .primary()
                            .bg(palette.accent)
                            .text_color(color(0xffffff))
                            .icon(IconName::SquareTerminal)
                            .label(self.localized("开始处理", "Start processing"))
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
                                    Button::new("enqueue")
                                        .primary()
                                        .bg(palette.accent)
                                        .text_color(color(0xffffff))
                                        .icon(IconName::Plus)
                                        .tooltip(self.localized("加入等待队列", "Add to queue"))
                                        .disabled(!self.can_enqueue())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.enqueue_selected_or_draft(cx)
                                        })),
                                )
                                .child(
                                    Button::new("cancel")
                                        .danger()
                                        .icon(IconName::CircleX)
                                        .tooltip(
                                            self.localized(
                                                "取消当前解算",
                                                "Cancel current solution",
                                            ),
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, _| this.cancel_active()),
                                        ),
                                )
                                .into_any_element()
                        } else if self.selected_run_id.is_some() {
                            Button::new("run")
                                .primary()
                                .bg(palette.accent)
                                .text_color(color(0xffffff))
                                .icon(IconName::Replace)
                                .tooltip(self.localized(
                                    "按此配置重新解算",
                                    "Reprocess with this configuration",
                                ))
                                .disabled(!self.can_enqueue())
                                .on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.enqueue_selected_or_draft(cx)
                                    }),
                                )
                                .into_any_element()
                        } else {
                            Button::new("workspace-new-analysis")
                                .primary()
                                .bg(palette.accent)
                                .text_color(color(0xffffff))
                                .icon(IconName::Plus)
                                .label(self.localized("新建分析", "New analysis"))
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
        _compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected_name = self.file_label();
        let page_context = match self.page {
            AppPage::NewAnalysis => self.localized("分析草稿", "Analysis draft"),
            AppPage::InterfaceSettings => self.localized("界面设置", "Interface settings"),
            AppPage::RunLibrary => self.localized("运行库", "Run library"),
            AppPage::Workspace => self.localized("选中的运行", "Selected run"),
        };

        TitleBar::new()
            .bg(palette.background)
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
                    .px(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .flex_shrink_0()
                            .child(
                                div()
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(palette.accent)
                                    .child(IconName::Map),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(palette.text)
                                    .child("GNSScope"),
                            ),
                    )
                    .child(div().h(px(18.)).w(px(1.)).bg(palette.border))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(7.))
                            .min_w_0()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(9.))
                                    .text_color(palette.muted)
                                    .child(page_context),
                            )
                            .child(
                                div()
                                    .max_w(px(420.))
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_size(px(11.))
                                    .font_family(data_font_family())
                                    .text_color(palette.text)
                                    .child(selected_name),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("global-interface-settings")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(if self.page == AppPage::InterfaceSettings {
                                palette.accent
                            } else {
                                palette.muted
                            })
                            .bg(if self.page == AppPage::InterfaceSettings {
                                palette.accent.opacity(0.10)
                            } else {
                                palette.background
                            })
                            .icon(IconName::Settings2)
                            .label(self.localized("设置", "Settings"))
                            .tooltip(self.localized("打开界面设置", "Open interface settings"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.page = AppPage::InterfaceSettings;
                                this.side_drawer = None;
                                this.focused_plot = None;
                                cx.notify();
                            })),
                    ),
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
                                .text_size(px(10.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                .font_weight(gpui::FontWeight::SEMIBOLD)
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
                    Progress::new()
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
                                            filter.label()
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
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "reveal-run-{id}"
                                                )))
                                                .ghost()
                                                .text_color(palette.muted)
                                                .icon(IconName::FolderOpen)
                                                .tooltip(self.localized(
                                                    "在文件管理器中定位",
                                                    "Reveal in file manager",
                                                ))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.reveal_source_file(id);
                                                    cx.notify();
                                                })),
                                            )
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "delete-run-{id}"
                                                )))
                                                .ghost()
                                                .text_color(palette.muted)
                                                .icon(IconName::Delete)
                                                .tooltip(self.localized(
                                                    "删除历史记录",
                                                    "Delete history entry",
                                                ))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.delete_historical_run(id);
                                                    cx.notify();
                                                })),
                                            ),
                                    )
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

    fn render_runs_dock(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let history_query = self.history_search.read(cx).value().to_string();
        let active = self.active_run.as_ref().map(|run| {
            let id = run.id;
            let selected = self.selected_run_id == Some(id);
            div()
                .id(SharedString::from(format!("active-run-{id}")))
                .px(px(12.))
                .py(px(9.))
                .cursor_pointer()
                .bg(if selected {
                    palette.accent.opacity(0.11)
                } else {
                    palette.sidebar
                })
                .border_l_2()
                .border_color(if selected {
                    palette.accent
                } else {
                    palette.sidebar
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.page = AppPage::Workspace;
                    this.select_run(id, window, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .child(div().size(px(6.)).rounded_full().bg(palette.healthy))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_size(px(10.))
                                .font_family(data_font_family())
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
                                .font_family(data_font_family())
                                .text_color(palette.healthy)
                                .child(format!("{:.0}%", run.progress * 100.0)),
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
                                Progress::new()
                                    .h(px(3.))
                                    .bg(palette.accent)
                                    .value((run.progress * 100.0) as f32),
                            ),
                        )
                        .child(
                            div()
                                .text_size(px(8.))
                                .text_color(palette.muted)
                                .child(self.localized("处理中", "Running")),
                        ),
                )
        });

        div()
            .w(px(286.))
            .min_w(px(254.))
            .h_full()
            .flex_shrink_0()
            .bg(palette.sidebar)
            .border_r_1()
            .border_color(palette.border)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(38.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(self.localized("运行", "Runs")),
                    )
                    .child(
                        Button::new("runs-new-analysis")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(palette.muted)
                            .icon(IconName::Plus)
                            .tooltip(self.localized("新建分析 · ⌘N", "New analysis · Cmd+N"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_new_task(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("runs-filter-menu")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(palette.muted)
                            .icon(IconName::Ellipsis)
                            .tooltip(self.localized("切换运行筛选", "Cycle run filter"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.history_filter = this.history_filter.next();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .h(px(42.))
                    .flex_shrink_0()
                    .px(px(9.))
                    .py(px(7.))
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        Input::new(&self.history_search)
                            .prefix(IconName::Search)
                            .bg(palette.background),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(run_section_header(
                        self.localized("当前运行", "Active"),
                        usize::from(self.active_run.is_some()),
                        palette,
                    ))
                    .children(active)
                    .children(self.active_run.is_none().then(|| {
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .text_size(px(9.))
                            .text_color(palette.muted)
                            .child(self.localized("没有正在处理的运行", "No active run"))
                    }))
                    .child(run_section_header(
                        self.localized("等待队列", "Queue"),
                        self.queue.len(),
                        palette,
                    ))
                    .children(self.queue.iter().enumerate().map(|(index, run)| {
                        let id = run.id;
                        div()
                            .min_h(px(42.))
                            .px(px(12.))
                            .py(px(7.))
                            .border_b_1()
                            .border_color(palette.border.opacity(0.55))
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(div().size(px(6.)).rounded_full().bg(palette.warning))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_size(px(9.))
                                            .font_family(data_font_family())
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
                                            .mt(px(2.))
                                            .text_size(px(8.))
                                            .text_color(palette.warning)
                                            .child(self.localized("已排队", "Queued")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        Button::new(SharedString::from(format!("dock-up-{id}")))
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
                                        Button::new(SharedString::from(format!("dock-down-{id}")))
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
                                            "dock-cancel-{id}"
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
                    .child(run_section_header(
                        self.localized("历史记录", "History"),
                        self.history
                            .iter()
                            .filter(|run| run.status != RunStatus::Queued)
                            .count(),
                        palette,
                    ))
                    .children(
                        self.history
                            .iter()
                            .filter(|run| {
                                run.status != RunStatus::Queued
                                    && Some(run.id)
                                        != self.active_run.as_ref().map(|active| active.id)
                                    && history_run_matches(run, &history_query, self.history_filter)
                            })
                            .take(80)
                            .map(|run| {
                                let id = run.id;
                                let selected = self.selected_run_id == Some(id);
                                let (status, status_color) = match run.status {
                                    RunStatus::Completed => {
                                        (self.localized("完成", "Completed"), palette.healthy)
                                    }
                                    RunStatus::Failed => {
                                        (self.localized("失败", "Failed"), palette.danger)
                                    }
                                    RunStatus::Cancelled => {
                                        (self.localized("已取消", "Cancelled"), palette.warning)
                                    }
                                    RunStatus::Interrupted => {
                                        (self.localized("已中断", "Interrupted"), palette.warning)
                                    }
                                    _ => (self.localized("处理中", "Processing"), palette.accent),
                                };
                                div()
                                    .id(SharedString::from(format!("dock-run-{id}")))
                                    .min_h(px(48.))
                                    .px(px(12.))
                                    .py(px(7.))
                                    .cursor_pointer()
                                    .bg(if selected {
                                        palette.accent.opacity(0.09)
                                    } else {
                                        palette.sidebar
                                    })
                                    .border_l_2()
                                    .border_b_1()
                                    .border_color(if selected {
                                        palette.accent
                                    } else {
                                        palette.border.opacity(0.55)
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.page = AppPage::Workspace;
                                        this.select_run(id, window, cx);
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(7.))
                                            .child(
                                                div().size(px(6.)).rounded_full().bg(status_color),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .text_size(px(9.))
                                                    .font_family(data_font_family())
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
                                                    .text_size(px(8.))
                                                    .text_color(status_color)
                                                    .child(status),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(3.))
                                            .ml(px(13.))
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(8.))
                                                    .font_family(data_font_family())
                                                    .text_color(palette.muted)
                                                    .child(format!(
                                                        "{} epochs · {:.0}%",
                                                        run.pvt_epoch_count,
                                                        run.progress * 100.0
                                                    )),
                                            )
                                            .children(selected.then(|| {
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .child(
                                                        Button::new(SharedString::from(format!(
                                                            "dock-reveal-{id}"
                                                        )))
                                                        .ghost()
                                                        .icon(IconName::FolderOpen)
                                                        .tooltip(self.localized(
                                                            "在文件管理器中定位",
                                                            "Reveal source file",
                                                        ))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.reveal_source_file(id);
                                                                cx.notify();
                                                            },
                                                        )),
                                                    )
                                                    .child(
                                                        Button::new(SharedString::from(format!(
                                                            "dock-delete-{id}"
                                                        )))
                                                        .ghost()
                                                        .icon(IconName::Delete)
                                                        .tooltip(self.localized(
                                                            "删除历史记录",
                                                            "Delete history entry",
                                                        ))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.delete_historical_run(id);
                                                                cx.notify();
                                                            },
                                                        )),
                                                    )
                                            })),
                                    )
                            }),
                    ),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .border_t_1()
                    .border_color(palette.border)
                    .flex()
                    .items_center()
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
                            format!("{} runs · {} queued", self.history.len(), self.queue.len())
                        }
                    }),
            )
            .into_any_element()
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
                            .text_color(color(0xffffff))
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
                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                    Progress::new()
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

    fn render_new_analysis(
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
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scrollbar()
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
                                    .font_weight(gpui::FontWeight::BOLD)
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
                                                color(0xffffff)
                                            } else {
                                                palette.muted
                                            })
                                            .child(number),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                                        self.localized(
                                                            "本地文件，不会上传",
                                                            "Local file, never uploaded",
                                                        ),
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
                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                                Button::new("new-analysis-choose-source")
                                                    .mt(px(12.))
                                                    .w_full()
                                                    .ghost()
                                                    .border_1()
                                                    .border_color(palette.border)
                                                    .icon(IconName::FolderOpen)
                                                    .label(if source_ready {
                                                        self.localized("更换文件", "Replace file")
                                                    } else {
                                                        self.localized("选择 IQ 文件", "Choose IQ file")
                                                    })
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
                                                self.localized(
                                                    "当前生产管线",
                                                    "Current production pipeline",
                                                ),
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
                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                                            .child("GPS L1 C/A"),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(7.))
                                                            .py(px(3.))
                                                            .rounded(px(3.))
                                                            .bg(palette.healthy.opacity(0.10))
                                                            .text_size(px(8.))
                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                                self.localized(
                                                    "默认值适用于常见离线记录",
                                                    "Defaults suit typical offline recordings",
                                                ),
                                                palette,
                                            ))
                                            .child(
                                                Button::new("new-analysis-reset")
                                                    .ghost()
                                                    .icon(IconName::Undo2)
                                                    .label(self.localized("恢复默认", "Reset"))
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.reset_receiver_settings(window, cx);
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    )
                                    .children((issue_count > 0).then(|| {
                                        div()
                                            .px(px(16.))
                                            .py(px(8.))
                                            .flex()
                                            .items_center()
                                            .gap(px(7.))
                                            .bg(palette.danger.opacity(0.08))
                                            .border_b_1()
                                            .border_color(palette.danger.opacity(0.30))
                                            .text_size(px(9.))
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
                                                    Button::new(SharedString::from(format!(
                                                        "new-analysis-category-{:?}",
                                                        candidate
                                                    )))
                                                    .ghost()
                                                    .icon(candidate.icon())
                                                    .label(candidate.label(self.interface_language))
                                                    .text_color(if selected {
                                                        palette.accent
                                                    } else {
                                                        palette.muted
                                                    })
                                                    .bg(if selected {
                                                        palette.accent.opacity(0.10)
                                                    } else {
                                                        palette.surface
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
                                                self.render_settings_category_content(
                                                    category, palette, cx,
                                                )
                                                .into_any_element(),
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
                                                Button::new("new-analysis-start")
                                                    .primary()
                                                    .bg(palette.accent)
                                                    .text_color(color(0xffffff))
                                                    .icon(IconName::SquareTerminal)
                                                    .label(self.localized(
                                                        "开始处理",
                                                        "Start processing",
                                                    ))
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

    fn render_run_library(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let history_query = self.history_search.read(cx).value().to_string();
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
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                div()
                    .w_full()
                    .max_w(px(1040.))
                    .mx_auto()
                    .px(if compact { px(16.) } else { px(28.) })
                    .py(px(24.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(palette.text)
                                            .child(self.localized("运行库", "Run library")),
                                    )
                                    .child(
                                        div()
                                            .mt(px(5.))
                                            .text_size(px(11.))
                                            .text_color(palette.muted)
                                            .child(self.localized(
                                                "查找历史分析、查看排队状态并重新打开结果。",
                                                "Find previous analyses, inspect the queue, and reopen results.",
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("library-new-analysis")
                                    .primary()
                                    .bg(palette.accent)
                                    .text_color(color(0xffffff))
                                    .icon(IconName::Plus)
                                    .label(self.localized("新建分析", "New analysis"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_new_task(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(20.))
                            .flex()
                            .when(compact, |this| this.flex_col())
                            .gap(px(8.))
                            .child(
                                div()
                                    .h(px(36.))
                                    .flex_1()
                                    .child(
                                        Input::new(&self.history_search)
                                            .prefix(IconName::Search)
                                            .bg(palette.surface),
                                    ),
                            )
                            .child(
                                Button::new("library-history-filter")
                                    .ghost()
                                    .border_1()
                                    .border_color(palette.border)
                                    .icon(IconName::SortDescending)
                                    .label(match (self.interface_language, self.history_filter) {
                                        (InterfaceLanguage::SimplifiedChinese, filter) => {
                                            filter.label()
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
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.history_filter = this.history_filter.next();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children((!self.queue.is_empty()).then(|| {
                        div()
                            .mt(px(18.))
                            .p(px(14.))
                            .rounded(px(7.))
                            .bg(palette.surface)
                            .border_1()
                            .border_color(palette.border)
                            .child(Self::section_title(
                                self.localized("等待队列", "Queue"),
                                match self.interface_language {
                                    InterfaceLanguage::SimplifiedChinese => {
                                        format!("{} 项等待处理", self.queue.len())
                                    }
                                    InterfaceLanguage::English => {
                                        format!("{} waiting", self.queue.len())
                                    }
                                },
                                palette,
                            ))
                            .children(self.queue.iter().enumerate().map(|(index, run)| {
                                let id = run.id;
                                div()
                                    .mt(px(8.))
                                    .px(px(10.))
                                    .py(px(8.))
                                    .rounded(px(5.))
                                    .bg(palette.surface_alt)
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_size(px(11.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(
                                                run.source_path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                                    .into_owned(),
                                            ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "library-queue-up-{id}"
                                        )))
                                        .ghost()
                                        .icon(IconName::ChevronUp)
                                        .disabled(index == 0)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.move_queued(id, -1);
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "library-queue-down-{id}"
                                        )))
                                        .ghost()
                                        .icon(IconName::ChevronDown)
                                        .disabled(index + 1 == self.queue.len())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.move_queued(id, 1);
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "library-queue-cancel-{id}"
                                        )))
                                        .ghost()
                                        .icon(IconName::CircleX)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.cancel_queued(id);
                                            cx.notify();
                                        })),
                                    )
                            }))
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
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(palette.muted)
                                    .child(self.localized("历史分析", "HISTORY")),
                            )
                            .child(
                                div()
                                    .text_size(px(9.))
                                    .text_color(palette.muted)
                                    .child(match self.interface_language {
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
                        let status_color = match run.status {
                            RunStatus::Completed => palette.healthy,
                            RunStatus::Failed => palette.danger,
                            _ => palette.warning,
                        };
                        div()
                            .mb(px(7.))
                            .px(px(14.))
                            .py(px(12.))
                            .rounded(px(6.))
                            .bg(palette.surface)
                            .border_1()
                            .border_color(if selected {
                                palette.accent
                            } else {
                                palette.border
                            })
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(34.))
                                    .flex_shrink_0()
                                    .rounded(px(5.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(status_color.opacity(0.10))
                                    .text_color(status_color)
                                    .child(IconName::ChartPie),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("library-open-run-{id}")))
                                    .flex_1()
                                    .min_w_0()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.page = AppPage::Workspace;
                                        this.select_run(id, window, cx);
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_size(px(12.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                            .mt(px(4.))
                                            .text_size(px(9.))
                                            .text_color(palette.muted)
                                            .child(format!(
                                                "{} · {} epochs · {:.0}%",
                                                run.created_at.format("%Y-%m-%d %H:%M"),
                                                run.pvt_epoch_count,
                                                run.progress * 100.0
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(7.))
                                    .py(px(3.))
                                    .rounded(px(3.))
                                    .bg(status_color.opacity(0.10))
                                    .text_size(px(8.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(status_color)
                                    .child(format!("{:?}", run.status)),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "library-reveal-run-{id}"
                                )))
                                .ghost()
                                .icon(IconName::FolderOpen)
                                .tooltip(self.localized(
                                    "在文件管理器中定位",
                                    "Reveal in file manager",
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.reveal_source_file(id);
                                    cx.notify();
                                })),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "library-delete-run-{id}"
                                )))
                                .ghost()
                                .icon(IconName::Delete)
                                .tooltip(self.localized("删除历史记录", "Delete history entry"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.delete_historical_run(id);
                                    cx.notify();
                                })),
                            )
                    }))
                    .children(visible_runs_empty.then(|| {
                        div()
                            .mt(px(8.))
                            .h(px(180.))
                            .rounded(px(7.))
                            .bg(palette.surface)
                            .border_1()
                            .border_color(palette.border)
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

    fn render_interface_setting_section(
        &self,
        icon: IconName,
        title: &'static str,
        description: &'static str,
        control: AnyElement,
        palette: Palette,
    ) -> impl IntoElement {
        div()
            .px(px(4.))
            .py(px(18.))
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(34.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .bg(palette.accent.opacity(0.12))
                            .text_color(palette.accent)
                            .child(icon),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(palette.text)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(10.))
                                    .text_color(palette.muted)
                                    .child(description),
                            ),
                    ),
            )
            .child(div().mt(px(18.)).child(control))
    }

    fn render_interface_settings(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let language = self.interface_language;
        let theme_control = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .children(
                [
                    (ThemePreference::System, IconName::Palette, "系统", "System"),
                    (ThemePreference::Light, IconName::Sun, "浅色", "Light"),
                    (ThemePreference::Dark, IconName::Moon, "深色", "Dark"),
                ]
                .into_iter()
                .map(|(preference, icon, chinese, english)| {
                    let active = self.theme_preference == preference;
                    let button = Button::new(SharedString::from(format!(
                        "interface-theme-{}",
                        preference.value()
                    )))
                    .icon(icon)
                    .label(match language {
                        InterfaceLanguage::SimplifiedChinese => chinese,
                        InterfaceLanguage::English => english,
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_theme_preference(preference, window, cx);
                        cx.notify();
                    }));
                    if active {
                        button.primary()
                    } else {
                        button
                            .ghost()
                            .text_color(palette.text)
                            .border_1()
                            .border_color(palette.border)
                    }
                }),
            )
            .into_any_element();

        let language_control = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .children(
                [
                    (InterfaceLanguage::SimplifiedChinese, "简体中文"),
                    (InterfaceLanguage::English, "English"),
                ]
                .into_iter()
                .map(|(option, label)| {
                    let active = language == option;
                    let button = Button::new(SharedString::from(format!(
                        "interface-language-{}",
                        option.value()
                    )))
                    .icon(IconName::Globe)
                    .label(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_interface_language(option, window, cx);
                        cx.notify();
                    }));
                    if active {
                        button.primary()
                    } else {
                        button
                            .ghost()
                            .text_color(palette.text)
                            .border_1()
                            .border_color(palette.border)
                    }
                }),
            )
            .into_any_element();

        let timeline_control = div()
            .flex()
            .items_center()
            .justify_between()
            .p(px(12.))
            .rounded(px(6.))
            .bg(palette.surface_alt)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.text)
                    .child(self.localized(
                        "在主界面底部固定显示 epoch 滑块",
                        "Keep the epoch slider fixed at the bottom of the workspace",
                    )),
            )
            .child(
                Button::new("interface-timeline-visible")
                    .icon(if self.timeline_visible {
                        IconName::Eye
                    } else {
                        IconName::EyeOff
                    })
                    .label(if self.timeline_visible {
                        self.localized("显示", "Visible")
                    } else {
                        self.localized("隐藏", "Hidden")
                    })
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_timeline_visibility(!this.timeline_visible);
                        cx.notify();
                    })),
            )
            .into_any_element();

        div()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                div()
                    .w_full()
                    .max_w(px(780.))
                    .mx_auto()
                    .px(px(28.))
                    .py(px(24.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(18.))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(palette.text)
                                            .child(self.localized(
                                                "界面设置",
                                                "Interface settings",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .mt(px(6.))
                                            .text_size(px(11.))
                                            .text_color(palette.muted)
                                            .child(self.localized(
                                                "统一管理 GNSScope 的外观与工作区显示偏好",
                                                "Manage GNSScope appearance and workspace display preferences",
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("close-interface-settings")
                                    .ghost()
                                    .icon(IconName::ArrowLeft)
                                    .tooltip(self.localized("返回工作台", "Back to workspace"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.page = AppPage::Workspace;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(18.))
                            .child(self.render_interface_setting_section(
                                IconName::Palette,
                                self.localized("外观主题", "Appearance theme"),
                                self.localized(
                                    "跟随系统，或固定使用浅色与深色主题。更改会立即应用。",
                                    "Follow the system or use a fixed light or dark theme. Changes apply immediately.",
                                ),
                                theme_control,
                                palette,
                            )),
                    )
                    .child(
                        div().child(self.render_interface_setting_section(
                                IconName::Globe,
                                self.localized("界面语言", "Interface language"),
                                self.localized(
                                    "选择界面导航与设置页面使用的语言。",
                                    "Choose the language used by interface navigation and settings.",
                                ),
                                language_control,
                                palette,
                            )),
                    )
                    .child(
                        div().child(self.render_interface_setting_section(
                                IconName::PanelBottom,
                                self.localized("Epoch 时间轴", "Epoch timeline"),
                                self.localized(
                                    "控制右侧主工作区底部的 epoch 选择滑块是否可见。",
                                    "Control whether the epoch selection slider is visible at the bottom of the workspace.",
                                ),
                                timeline_control,
                                palette,
                            )),
                    ),
            )
            .into_any_element()
    }

    fn render_progress(&self, palette: Palette) -> impl IntoElement {
        let run = self.active_run.as_ref().or_else(|| {
            self.selected_run_id
                .and_then(|id| self.history.iter().find(|run| run.id == id))
        });
        let progress = run.map(|run| run.progress).unwrap_or(0.0);
        let elapsed = run
            .map(|run| run.processed_samples as f64 / run.settings.sample_rate_hz)
            .unwrap_or(0.0);
        let total = run
            .map(|run| run.total_samples as f64 / run.settings.sample_rate_hz)
            .unwrap_or(0.0);
        let status_message = if self.active_run.is_some() {
            self.status_message.clone()
        } else {
            run.map(|run| run.message.clone())
                .unwrap_or_else(|| self.status_message.clone())
        };
        let status_color = if self.active_run.is_some() {
            palette.accent
        } else if progress >= 1.0 {
            palette.healthy
        } else {
            palette.muted
        };
        div()
            .px(px(16.))
            .pt(px(8.))
            .pb(px(7.))
            .bg(palette.surface_alt)
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .min_w_0()
                            .child(div().size(px(6.)).rounded(px(2.)).bg(status_color))
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(palette.text)
                                    .child(status_message),
                            ),
                    )
                    .child(
                        div()
                            .ml(px(16.))
                            .flex()
                            .items_baseline()
                            .gap(px(12.))
                            .font_family(data_font_family())
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(palette.muted)
                                    .child(format!("{elapsed:.1} / {total:.1} s")),
                            )
                            .child(
                                div()
                                    .min_w(px(54.))
                                    .text_right()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(palette.accent)
                                    .child(format!("{:.1}%", progress * 100.0)),
                            ),
                    ),
            )
            .child(
                Progress::new()
                    .mt(px(6.))
                    .h(px(2.))
                    .bg(palette.accent)
                    .value((progress * 100.0) as f32),
            )
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
            .mt(px(10.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(10.))
                    .text_color(if issue.is_some() {
                        palette.danger
                    } else {
                        palette.muted
                    })
                    .child(label)
                    .child(unit),
            )
            .child(
                div()
                    .mt(px(4.))
                    .h(px(34.))
                    .child(Input::new(input).bg(palette.background)),
            )
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
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                Button::new("reset-receiver-settings")
                                    .ghost()
                                    .icon(IconName::Undo2)
                                    .tooltip(self.localized(
                                        "恢复接收机默认参数，但保留当前 IQ 文件",
                                        "Restore receiver defaults while keeping the current IQ file",
                                    ))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.reset_receiver_settings(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("close-settings-drawer")
                                    .ghost()
                                    .icon(IconName::CircleX)
                                    .tooltip(self.localized(
                                        "关闭接收机配置检查器",
                                        "Close receiver inspector",
                                    ))
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
                                let button = Button::new(SharedString::from(format!(
                                    "settings-category-{:?}",
                                    candidate
                                )))
                                .w_full()
                                .ghost()
                                .justify_start()
                                .icon(candidate.icon())
                                .text_color(if selected {
                                    palette.accent
                                } else {
                                    palette.muted
                                })
                                .bg(if selected {
                                    palette.accent.opacity(0.10)
                                } else {
                                    palette.sidebar
                                })
                                .tooltip(candidate.description(self.interface_language))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_category = candidate;
                                    cx.notify();
                                }));
                                if icon_only_categories {
                                    button
                                } else {
                                    button.label(candidate.label(self.interface_language))
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
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
        div()
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
                        Button::new("drawer-iq-format")
                            .ghost()
                            .icon(IconName::Replace)
                            .label(format!("{:?}", self.settings.iq_format))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_iq_format();
                                cx.notify();
                            })),
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
            .mt(px(18.))
            .pt(px(12.))
            .border_t_1()
            .border_color(palette.border)
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(palette.text)
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
            .mt(px(8.))
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
                Switch::new(SharedString::from(format!("{field}-toggle")))
                    .checked(enabled)
                    .tooltip(if enabled {
                        self.localized("已开启", "Enabled")
                    } else {
                        self.localized("已关闭", "Disabled")
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
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

    fn render_workspace_tabs(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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

        div()
            .h(px(82.))
            .flex_shrink_0()
            .bg(palette.surface)
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .h(px(43.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_size(px(13.))
                                            .font_family(data_font_family())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(palette.text)
                                            .child(self.file_label()),
                                    )
                                    .child(div().size(px(6.)).rounded_full().bg(status_color))
                                    .child(
                                        div()
                                            .text_size(px(8.))
                                            .text_color(status_color)
                                            .child(status_label),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(3.))
                                    .text_size(px(8.))
                                    .font_family(data_font_family())
                                    .text_color(palette.muted)
                                    .child(format!(
                                        "GPS L1 C/A  ·  {:.3} Msps  ·  IF {:.3} MHz  ·  {:?}",
                                        self.settings.sample_rate_hz / 1e6,
                                        self.settings.intermediate_frequency_hz / 1e6,
                                        self.settings.iq_format
                                    )),
                            ),
                    )
                    .children((self.active_run.is_some() && !self.follow_live).then(|| {
                        Button::new("workspace-return-live")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(palette.accent)
                            .icon(IconName::Redo2)
                            .label(self.localized("返回实时", "Return live"))
                            .tooltip(self.localized(
                                "切回正在处理的运行和最新 epoch",
                                "Return to the active run and newest epoch",
                            ))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.return_to_live(window, cx);
                                cx.notify();
                            }))
                    }))
                    .child(
                        Button::new("workspace-event-log")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(palette.muted)
                            .icon(IconName::Inbox)
                            .label(self.localized("事件日志", "Event log"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.workspace_view = WorkspaceView::Navigation;
                                this.focused_plot = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("workspace-run-configuration")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(if self.side_drawer == Some(SideDrawer::Settings) {
                                palette.accent
                            } else {
                                palette.muted
                            })
                            .icon(IconName::Settings2)
                            .label(self.localized("运行配置", "Run configuration"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let drawer =
                                    toggled_side_drawer(this.side_drawer, SideDrawer::Settings);
                                if drawer == Some(SideDrawer::Settings) {
                                    this.sync_setting_inputs(window, cx);
                                }
                                this.side_drawer = drawer;
                                cx.notify();
                            })),
                    )
                    .children(selected_is_active.then(|| {
                        Button::new("workspace-cancel-active")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(palette.danger)
                            .icon(IconName::CircleX)
                            .tooltip(self.localized("取消当前处理", "Cancel active run"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_active();
                                cx.notify();
                            }))
                    }))
                    .children((selected_run.is_some() && !selected_is_active).then(|| {
                        Button::new("workspace-reprocess")
                            .ghost()
                            .rounded(px(3.))
                            .text_color(palette.muted)
                            .icon(IconName::Replace)
                            .tooltip(self.localized(
                                "使用此运行的配置重新处理",
                                "Reprocess with this run configuration",
                            ))
                            .disabled(!self.can_enqueue())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.enqueue_selected_or_draft(cx);
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .h(px(39.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(1.))
                    .children(WorkspaceView::ALL.into_iter().map(|view| {
                        let selected = self.workspace_view == view;
                        Button::new(SharedString::from(format!("workbench-view-{}", view.id())))
                            .h_full()
                            .ghost()
                            .rounded(px(0.))
                            .label(view.label(self.interface_language))
                            .text_color(if selected {
                                palette.text
                            } else {
                                palette.muted
                            })
                            .bg(palette.surface)
                            .border_b_2()
                            .border_color(if selected {
                                palette.accent
                            } else {
                                palette.surface
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
                            .font_weight(gpui::FontWeight::BOLD)
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
                                Button::new("empty-new-analysis")
                                    .primary()
                                    .bg(palette.accent)
                                    .text_color(color(0xffffff))
                                    .icon(IconName::Plus)
                                    .label(self.localized("新建分析", "New analysis"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_new_task(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("empty-open-library")
                                    .ghost()
                                    .border_1()
                                    .border_color(palette.border)
                                    .icon(IconName::ChartPie)
                                    .label(self.localized("打开运行库", "Open run library"))
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
        let valid_pvt = pvt.filter(|pvt| pvt_position_is_displayable(pvt));
        let latitude = valid_pvt
            .map(|p| format!("{:.7}", p.latitude_deg))
            .unwrap_or_else(|| "--".to_string());
        let longitude = valid_pvt
            .map(|p| format!("{:.7}", p.longitude_deg))
            .unwrap_or_else(|| "--".to_string());
        let altitude = valid_pvt
            .map(|p| format!("{:.2}", p.altitude_m))
            .unwrap_or_else(|| "--".to_string());
        let speed = valid_pvt
            .and_then(|p| p.velocity.as_ref())
            .map(|v| format!("{:.3}", v.speed_mps))
            .unwrap_or_else(|| "--".to_string());
        let clock = valid_pvt
            .and_then(|p| p.time.receiver_clock_drift_ppm)
            .map(|v| format!("{v:.4}"))
            .unwrap_or_else(|| "--".to_string());
        let hdop = valid_pvt
            .map(|p| format!("{:.2}", p.quality.hdop))
            .unwrap_or_else(|| "--".to_string());
        let utc = pvt
            .and_then(|p| p.time.utc.as_ref())
            .map(|utc| {
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:06.3} UTC",
                    utc.year, utc.month, utc.day, utc.hour, utc.minute, utc.second
                )
            })
            .unwrap_or_else(|| "UTC unavailable".to_string());

        div()
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .p(px(10.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(Self::section_title(
                        self.localized("当前 PVT", "Current PVT"),
                        if valid_pvt.is_some() {
                            pvt.map(|p| format!("{:?}", p.solution_kind))
                                .unwrap_or_else(|| "等待解算".to_string())
                        } else if pvt.is_some() {
                            self.localized("质量门限拒绝", "Rejected by quality gate")
                                .to_string()
                        } else {
                            self.localized("等待解算", "Waiting for solution")
                                .to_string()
                        },
                        palette,
                    ))
                    .child(
                        Button::new("candidate-comparison")
                            .ghost()
                            .icon(IconName::ChartPie)
                            .tooltip(self.localized(
                                "打开 Raw、Smoothed 与 Kalman 候选解的详细比较",
                                "Compare Raw, Smoothed, and Kalman candidates",
                            ))
                            .disabled(pvt.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.workspace_view = WorkspaceView::Candidates;
                                this.side_drawer = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .mt(px(6.))
                    .h(px(22.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .bg(palette.surface_alt)
                    .border_1()
                    .border_color(palette.border)
                    .text_size(px(7.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(palette.muted)
                    .child(div().w(px(96.)).child("PARAMETER"))
                    .child(div().flex_1().child("VALUE"))
                    .child(div().w(px(52.)).child("UNITS"))
                    .child(div().w(px(44.)).text_right().child("STATUS")),
            )
            .child(pvt_table_row(
                self.localized("纬度", "Latitude"),
                latitude,
                "deg",
                valid_pvt.is_some(),
                palette,
            ))
            .child(pvt_table_row(
                self.localized("经度", "Longitude"),
                longitude,
                "deg",
                valid_pvt.is_some(),
                palette,
            ))
            .child(pvt_table_row(
                self.localized("椭球高", "Ellipsoid height"),
                altitude,
                "m",
                valid_pvt.is_some(),
                palette,
            ))
            .child(pvt_table_row(
                self.localized("三维速度", "3D speed"),
                speed,
                "m/s",
                valid_pvt.is_some(),
                palette,
            ))
            .child(pvt_table_row(
                self.localized("接收机钟漂", "Clock drift"),
                clock,
                "ppm",
                valid_pvt.is_some(),
                palette,
            ))
            .child(pvt_table_row(
                "HDOP",
                hdop,
                "-",
                valid_pvt.is_some(),
                palette,
            ))
            .child(
                div()
                    .mt(px(4.))
                    .pt(px(7.))
                    .border_t_1()
                    .border_color(palette.border)
                    .text_size(px(9.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(palette.text)
                    .child(utc),
            )
            .into_any_element()
    }

    fn render_epoch_inspector(&self, palette: Palette) -> AnyElement {
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
            .w(px(298.))
            .min_w(px(280.))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scrollbar()
            .bg(palette.surface)
            .border_l_1()
            .border_color(palette.border)
            .child(
                div()
                    .min_h(px(56.))
                    .px(px(14.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(palette.text)
                                    .child(format!("Epoch {epoch_number}")),
                            )
                            .child(
                                div()
                                    .text_size(px(8.))
                                    .font_family(data_font_family())
                                    .text_color(palette.muted)
                                    .child(format!("{epoch_number} / {epoch_count}")),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(8.))
                            .font_family(data_font_family())
                            .text_color(palette.muted)
                            .child(self.file_label()),
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
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
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
                        self.display_diagnostic_events().len().to_string(),
                        palette,
                    ))
                    .children(self.display_diagnostic_events().last().map(|event| {
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
                    })),
            )
            .into_any_element()
    }

    fn render_channel_table(&self, palette: Palette) -> AnyElement {
        let channels = self.display_channels();
        let locked = channels
            .iter()
            .filter(|channel| channel.tracking_locked)
            .count();
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
                        format!("{locked} / {} locked", channels.len()),
                        palette,
                    )),
            )
            .child(channel_table_header(palette))
            .children(
                channels
                    .iter()
                    .map(|channel| channel_table_row(channel, palette)),
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
                        match self.interface_language {
                            InterfaceLanguage::SimplifiedChinese => {
                                format!("已持久化 {} 条", events.len())
                            }
                            InterfaceLanguage::English => format!("{} persisted", events.len()),
                        },
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
        let pvt = self.current_pvt();
        let valid_pvt = pvt.filter(|pvt| pvt_position_is_displayable(pvt));
        let satellites = valid_pvt
            .map(|pvt| pvt.satellites.as_slice())
            .unwrap_or(&[]);
        let _ = viewport_height;
        if let Some(focus) = self.focused_plot {
            let (title, subtitle, plot) = match focus {
                PlotFocus::Sky => (
                    "卫星天空图".to_string(),
                    "真实方位/仰角".to_string(),
                    sky_plot(
                        satellites,
                        channels,
                        self.hovered_prn,
                        self.pinned_prn,
                        palette,
                        cx,
                    )
                    .into_any_element(),
                ),
                PlotFocus::Position => (
                    "ENU 位置".to_string(),
                    "首个有效 epoch 为原点".to_string(),
                    position_plot(epochs, self.selected_epoch, palette).into_any_element(),
                ),
                PlotFocus::Cn0 => (
                    "C/N0 (dB-Hz) · GPS L1 C/A".to_string(),
                    format!("最近 {} 个 epoch", epoch_channels.len()),
                    cn0_plot(
                        epoch_channels,
                        channels,
                        self.selected_epoch,
                        self.hovered_prn,
                        self.pinned_prn,
                        palette,
                        cx,
                    )
                    .into_any_element(),
                ),
                PlotFocus::Velocity => (
                    "速度与钟漂".to_string(),
                    "共享 epoch 时间轴".to_string(),
                    velocity_plot(epochs, self.selected_epoch, palette).into_any_element(),
                ),
                PlotFocus::Altitude => (
                    "高度历史".to_string(),
                    altitude_statistics(epochs),
                    altitude_plot(epochs, self.selected_epoch, palette).into_any_element(),
                ),
            };
            return div()
                .px(px(12.))
                .pb(px(20.))
                .child(self.plot_panel(
                    (title, subtitle),
                    plot,
                    palette,
                    focus,
                    PlotPanelLayout::Expanded,
                    cx,
                ))
                .into_any_element();
        }
        let content = match self.workspace_view {
            WorkspaceView::Overview => div()
                .grid()
                .grid_cols(if compact { 1 } else { 2 })
                .gap(px(6.))
                .child(self.plot_panel(
                    ("卫星天空图", "真实方位/仰角"),
                    sky_plot(
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
                    cx,
                ))
                .child(self.plot_panel(
                    ("ENU 位置", "首个有效 epoch 为原点"),
                    position_plot(epochs, self.selected_epoch, palette),
                    palette,
                    PlotFocus::Position,
                    PlotPanelLayout::Standard,
                    cx,
                ))
                .child(self.render_pvt_summary(palette, false, cx))
                .child(self.plot_panel(
                    (
                        "C/N0 (dB-Hz) · GPS L1 C/A",
                        format!("最近 {} 个 epoch", epoch_channels.len()),
                    ),
                    cn0_plot(
                        epoch_channels,
                        channels,
                        self.selected_epoch,
                        self.hovered_prn,
                        self.pinned_prn,
                        palette,
                        cx,
                    ),
                    palette,
                    PlotFocus::Cn0,
                    PlotPanelLayout::Standard,
                    cx,
                ))
                .into_any_element(),
            WorkspaceView::Channels => div()
                .child(self.plot_panel(
                    (
                        "稳定通道 C/N0 历史",
                        format!("{} epochs · 最多 8 个 Healthy PRN", epoch_channels.len()),
                    ),
                    cn0_plot(
                        epoch_channels,
                        channels,
                        self.selected_epoch,
                        self.hovered_prn,
                        self.pinned_prn,
                        palette,
                        cx,
                    ),
                    palette,
                    PlotFocus::Cn0,
                    PlotPanelLayout::Standard,
                    cx,
                ))
                .child(div().mt(px(6.)).child(self.render_channel_table(palette)))
                .into_any_element(),
            WorkspaceView::Position => div()
                .child(self.plot_panel(
                    ("ENU 位置", "首个有效 epoch 为原点"),
                    position_plot(epochs, self.selected_epoch, palette),
                    palette,
                    PlotFocus::Position,
                    PlotPanelLayout::Standard,
                    cx,
                ))
                .child(
                    div()
                        .mt(px(6.))
                        .grid()
                        .grid_cols(if compact { 1 } else { 2 })
                        .gap(px(6.))
                        .child(self.plot_panel(
                            ("速度与钟漂", "共享 epoch 时间轴"),
                            velocity_plot(epochs, self.selected_epoch, palette),
                            palette,
                            PlotFocus::Velocity,
                            PlotPanelLayout::Standard,
                            cx,
                        ))
                        .child(self.plot_panel(
                            ("高度历史", altitude_statistics(epochs)),
                            altitude_plot(epochs, self.selected_epoch, palette),
                            palette,
                            PlotFocus::Altitude,
                            PlotPanelLayout::Standard,
                            cx,
                        )),
                )
                .into_any_element(),
            WorkspaceView::Navigation => self.render_navigation_events(palette),
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

        div()
            .px(px(12.))
            .pt(px(10.))
            .pb(px(20.))
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
                                    .font_weight(gpui::FontWeight::BOLD)
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
                        Button::new("close-candidate-comparison")
                            .ghost()
                            .icon(IconName::CircleX)
                            .tooltip("关闭候选解比较")
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
            .children(
                [
                    PvtEpochSolutionKind::Raw,
                    PvtEpochSolutionKind::Smoothed,
                    PvtEpochSolutionKind::Kalman,
                ]
                .into_iter()
                .map(|kind| self.render_candidate_card(pvt, kind, palette)),
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
        div()
            .mt(px(12.))
            .rounded(px(6.))
            .border_1()
            .border_color(if disposition == PvtCandidateDisposition::Selected {
                palette.accent
            } else {
                palette.border
            })
            .bg(palette.surface_alt)
            .p(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(palette.text)
                            .child(candidate_kind_label(kind)),
                    )
                    .child(
                        div()
                            .px(px(7.))
                            .py(px(3.))
                            .rounded(px(4.))
                            .bg(status_color.opacity(0.12))
                            .text_size(px(9.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
        let max_event_ms = events.last().map(|event| event.now_ms).unwrap_or(1).max(1);
        let markers = events
            .iter()
            .rev()
            .take(32)
            .enumerate()
            .map(|(index, event)| {
                let percent = event.now_ms as f32 / max_event_ms as f32;
                let (label, marker_color) = match event.kind {
                    GuiDiagnosticEventKind::Acquisition => ("ACQ", palette.healthy),
                    GuiDiagnosticEventKind::AcquisitionFailed => ("FAIL", palette.danger),
                    GuiDiagnosticEventKind::Reacquisition => ("REACQ", palette.accent),
                    GuiDiagnosticEventKind::ReacquisitionFailed => ("LOSS", palette.danger),
                    GuiDiagnosticEventKind::ChannelState => ("TRK", palette.accent),
                    GuiDiagnosticEventKind::Navigation => ("NAV", palette.warning),
                };
                (index, percent, label, marker_color)
            })
            .collect::<Vec<_>>();
        let epoch_number = self.selected_epoch.map(|index| index + 1).unwrap_or(0);
        let epoch_count = epochs.len();
        div()
            .flex_shrink_0()
            .h(px(112.))
            .bg(palette.surface)
            .border_t_1()
            .border_color(palette.border)
            .child(
                div()
                    .h(px(30.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(9.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(self.localized("选中运行时间线", "Selected run timeline")),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_family(data_font_family())
                            .text_color(palette.accent)
                            .child(format!("{epoch_number} / {epoch_count}")),
                    ),
            )
            .child(
                div()
                    .h(px(82.))
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(12.))
                            .child(
                                div()
                                    .relative()
                                    .h(px(38.))
                                    .border_b_1()
                                    .border_color(palette.border)
                                    .children(markers.into_iter().map(
                                        |(index, percent, label, color)| {
                                            div()
                                                .id(SharedString::from(format!(
                                                    "timeline-marker-{index}"
                                                )))
                                                .absolute()
                                                .left(gpui::relative(percent))
                                                .bottom(px(0.))
                                                .ml(px(-10.))
                                                .w(px(20.))
                                                .h(px(30.))
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .justify_end()
                                                .text_size(px(6.))
                                                .font_family(data_font_family())
                                                .text_color(color)
                                                .child(label)
                                                .child(
                                                    div()
                                                        .mt(px(3.))
                                                        .size(px(5.))
                                                        .rounded_full()
                                                        .bg(color),
                                                )
                                                .child(div().w(px(1.)).h(px(7.)).bg(color))
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .h(px(42.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(8.))
                                            .font_family(data_font_family())
                                            .text_color(palette.muted)
                                            .child("0"),
                                    )
                                    .child(
                                        div().flex_1().child(
                                            Slider::new(&self.timeline)
                                                .bg(palette.border)
                                                .text_color(palette.accent)
                                                .disabled(epoch_count < 2),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(8.))
                                            .font_family(data_font_family())
                                            .text_color(palette.muted)
                                            .child(epoch_count.to_string()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .w(px(196.))
                            .h_full()
                            .flex_shrink_0()
                            .border_l_1()
                            .border_color(palette.border)
                            .px(px(10.))
                            .py(px(8.))
                            .child(
                                div()
                                    .h(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        Button::new("epoch-previous")
                                            .ghost()
                                            .rounded(px(2.))
                                            .icon(IconName::ChevronLeft)
                                            .tooltip(
                                                self.localized("上一个 epoch", "Previous epoch"),
                                            )
                                            .disabled(epoch_count == 0)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.step_selected_epoch(-1, window, cx);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .font_family(data_font_family())
                                            .text_size(px(9.))
                                            .text_color(palette.text)
                                            .child(format!("{epoch_number:04}")),
                                    )
                                    .child(
                                        Button::new("epoch-next")
                                            .ghost()
                                            .rounded(px(2.))
                                            .icon(IconName::ChevronRight)
                                            .tooltip(self.localized("下一个 epoch", "Next epoch"))
                                            .disabled(epoch_count == 0)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.step_selected_epoch(1, window, cx);
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                Button::new("epoch-follow-live")
                                    .mt(px(4.))
                                    .w_full()
                                    .ghost()
                                    .rounded(px(2.))
                                    .border_1()
                                    .border_color(if self.follow_live {
                                        palette.accent
                                    } else {
                                        palette.border
                                    })
                                    .text_color(if self.follow_live {
                                        palette.accent
                                    } else {
                                        palette.muted
                                    })
                                    .bg(if self.follow_live {
                                        palette.accent.opacity(0.08)
                                    } else {
                                        palette.surface
                                    })
                                    .icon(IconName::Redo2)
                                    .label(self.localized("跟随实时", "Follow live"))
                                    .disabled(self.live_run_id.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.return_to_live(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }

    fn render_status_bar(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active_run.as_ref();
        let tracking_count = self
            .channels
            .iter()
            .filter(|channel| channel.tracking_locked)
            .count();
        let nav_count = self
            .channels
            .iter()
            .filter(|channel| channel.nav_locked)
            .count();
        let fix_label = self
            .epochs
            .last()
            .map(|pvt| format!("{:?}", pvt.solution_kind))
            .unwrap_or_else(|| "No fix".to_string());
        let active_label = active
            .map(|run| format!("{:.0}%", run.progress * 100.0))
            .unwrap_or_else(|| "Idle".to_string());

        div()
            .h(px(28.))
            .flex_shrink_0()
            .bg(palette.background)
            .border_t_1()
            .border_color(palette.border)
            .flex()
            .items_center()
            .text_size(px(8.))
            .text_color(palette.muted)
            .child(
                Button::new("toggle-runs-sidebar")
                    .ghost()
                    .rounded(px(0.))
                    .text_color(palette.muted)
                    .icon(if self.runs_sidebar_visible {
                        IconName::PanelLeftClose
                    } else {
                        IconName::PanelLeftOpen
                    })
                    .tooltip(if self.runs_sidebar_visible {
                        self.localized("折叠运行栏 · ⌘B", "Collapse Runs dock · Cmd+B")
                    } else {
                        self.localized("展开运行栏 · ⌘B", "Expand Runs dock · Cmd+B")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_runs_sidebar_visibility(!this.runs_sidebar_visible);
                        cx.notify();
                    })),
            )
            .child(status_bar_item(
                self.localized("活动运行", "Active run"),
                active_label,
                if active.is_some() {
                    palette.healthy
                } else {
                    palette.muted
                },
                palette,
            ))
            .child(status_bar_item(
                "SIGNAL",
                "GPS L1 C/A".to_string(),
                palette.text,
                palette,
            ))
            .child(status_bar_item(
                "RATE",
                format!("{:.3} MHz", self.settings.sample_rate_hz / 1e6),
                palette.text,
                palette,
            ))
            .child(status_bar_item(
                "TRACK",
                tracking_count.to_string(),
                palette.text,
                palette,
            ))
            .child(status_bar_item(
                "NAV",
                nav_count.to_string(),
                palette.text,
                palette,
            ))
            .child(status_bar_item(
                "FIX",
                fix_label,
                if self.epochs.is_empty() {
                    palette.muted
                } else {
                    palette.healthy
                },
                palette,
            ))
            .child(div().flex_1())
            .child(
                div()
                    .px(px(10.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .font_family(data_font_family())
                    .child(self.status_message.clone()),
            )
    }

    fn plot_panel(
        &self,
        header: (impl Into<SharedString>, impl Into<SharedString>),
        plot: impl IntoElement,
        palette: Palette,
        focus: PlotFocus,
        layout: PlotPanelLayout,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (title, subtitle) = header;
        let expanded = matches!(layout, PlotPanelLayout::Expanded);
        div()
            .min_h(if expanded { px(580.) } else { px(246.) })
            .rounded(px(0.))
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .p(px(12.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(Self::section_title(title, subtitle, palette))
                    .child(
                        Button::new(SharedString::from(format!("plot-focus-{}", focus.id())))
                            .ghost()
                            .icon(if expanded {
                                IconName::Minimize
                            } else {
                                IconName::Maximize
                            })
                            .tooltip(if expanded {
                                "退出聚焦"
                            } else {
                                "聚焦图表"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.workspace_view = WorkspaceView::for_plot(focus);
                                this.focused_plot = if expanded { None } else { Some(focus) };
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .h(if expanded { px(514.) } else { px(200.) })
                    .child(plot),
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
        let compact = viewport_width < 1_180.0;
        let compact_drawer = viewport_width < 900.0;
        let show_workspace = !compact_drawer || self.side_drawer.is_none();
        let show_epoch_inspector = viewport_width >= 1_320.0
            && self.side_drawer.is_none()
            && self.page == AppPage::Workspace;
        let workspace_empty = self.selected_run_id.is_none()
            && self.active_run.is_none()
            && self.display_epochs().is_empty();
        div()
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .font_family(interface_font_family())
            .flex()
            .flex_col()
            .child(self.render_title_bar(palette, compact, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .children(
                        self.runs_sidebar_visible
                            .then(|| self.render_runs_dock(palette, cx)),
                    )
                    .child(match self.page {
                        AppPage::NewAnalysis => self
                            .render_new_analysis(palette, compact, cx)
                            .into_any_element(),
                        AppPage::RunLibrary => self
                            .render_run_library(palette, compact, cx)
                            .into_any_element(),
                        AppPage::InterfaceSettings => self
                            .render_interface_settings(palette, cx)
                            .into_any_element(),
                        AppPage::Workspace => div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .flex()
                                    .children(show_workspace.then(|| {
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
                                                .child(self.render_workspace_tabs(palette, cx))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_h_0()
                                                        .overflow_y_scrollbar()
                                                        .child(self.render_progress(palette))
                                                        .child(self.render_overview(
                                                            palette,
                                                            compact,
                                                            viewport_height,
                                                            cx,
                                                        )),
                                                )
                                                .into_any_element()
                                        }
                                    }))
                                    .children(match self.side_drawer {
                                        Some(SideDrawer::Settings) => Some(
                                            self.render_config_drawer(
                                                palette,
                                                compact,
                                                compact_drawer,
                                                cx,
                                            )
                                            .into_any_element(),
                                        ),
                                        Some(SideDrawer::CandidateComparison) => Some(
                                            self.render_candidate_drawer(
                                                palette,
                                                compact,
                                                compact_drawer,
                                                cx,
                                            )
                                            .into_any_element(),
                                        ),
                                        None => show_epoch_inspector
                                            .then(|| self.render_epoch_inspector(palette)),
                                    }),
                            )
                            .children(
                                (self.timeline_visible && !workspace_empty)
                                    .then(|| self.render_timeline_bar(palette, cx)),
                            )
                            .into_any_element(),
                    }),
            )
            .child(self.render_status_bar(palette, cx))
    }
}

fn run_section_header(label: &'static str, count: usize, palette: Palette) -> impl IntoElement {
    div()
        .h(px(30.))
        .px(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .bg(palette.surface)
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(8.))
                .font_family(data_font_family())
                .text_color(palette.muted)
                .child(count.to_string()),
        )
}

fn inspector_section(label: &'static str, palette: Palette) -> impl IntoElement {
    div()
        .mt(px(12.))
        .pt(px(8.))
        .border_t_1()
        .border_color(palette.border)
        .text_size(px(9.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(palette.healthy)
        .child(label)
}

fn inspector_row(
    label: &'static str,
    value: impl Into<SharedString>,
    palette: Palette,
) -> impl IntoElement {
    div()
        .min_h(px(23.))
        .flex()
        .items_center()
        .child(
            div()
                .w(px(118.))
                .flex_shrink_0()
                .text_size(px(8.))
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_right()
                .text_size(px(8.))
                .font_family(data_font_family())
                .text_color(palette.text)
                .child(value.into()),
        )
}

fn status_bar_item(
    label: &'static str,
    value: String,
    value_color: Hsla,
    palette: Palette,
) -> impl IntoElement {
    div()
        .h_full()
        .px(px(10.))
        .border_l_1()
        .border_color(palette.border)
        .flex()
        .items_center()
        .gap(px(5.))
        .child(
            div()
                .text_size(px(7.))
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(8.))
                .font_family(data_font_family())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(value_color)
                .child(value),
        )
}

fn pvt_table_row(
    label: &'static str,
    value: String,
    units: &'static str,
    valid: bool,
    palette: Palette,
) -> impl IntoElement {
    div()
        .h(px(23.))
        .px(px(8.))
        .flex()
        .items_center()
        .border_l_1()
        .border_r_1()
        .border_b_1()
        .border_color(palette.border)
        .text_size(px(8.))
        .child(div().w(px(96.)).text_color(palette.muted).child(label))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .font_family(data_font_family())
                .text_color(palette.text)
                .child(value),
        )
        .child(
            div()
                .w(px(52.))
                .font_family(data_font_family())
                .text_color(palette.muted)
                .child(units),
        )
        .child(
            div()
                .w(px(44.))
                .text_right()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(if valid {
                    palette.healthy
                } else {
                    palette.muted
                })
                .child(if valid { "VALID" } else { "--" }),
        )
}

fn table_cell(
    value: impl Into<SharedString>,
    width: f32,
    palette: Palette,
    muted: bool,
) -> impl IntoElement {
    div()
        .w(px(width))
        .flex_shrink_0()
        .text_size(px(10.))
        .font_family(data_font_family())
        .text_color(if muted { palette.muted } else { palette.text })
        .child(value.into())
}

fn channel_table_header(palette: Palette) -> impl IntoElement {
    div()
        .h(px(30.))
        .px(px(14.))
        .flex()
        .items_center()
        .bg(palette.surface_alt)
        .border_b_1()
        .border_color(palette.border)
        .child(table_cell("PRN", 54.0, palette, true))
        .child(table_cell("STATE", 110.0, palette, true))
        .child(table_cell("C/N0 dB-Hz", 96.0, palette, true))
        .child(table_cell("DOPPLER Hz", 104.0, palette, true))
        .child(table_cell("RESIDUAL m", 104.0, palette, true))
}

fn channel_table_row(channel: &GuiChannelStatus, palette: Palette) -> impl IntoElement {
    let (state, state_color) = if channel.nav_locked {
        ("NAV LOCK", palette.healthy)
    } else if channel.tracking_locked {
        ("TRACKING", palette.accent)
    } else if channel.acquired {
        ("ACQUIRED", palette.warning)
    } else {
        ("SEARCH", palette.muted)
    };
    div()
        .min_h(px(34.))
        .px(px(14.))
        .flex()
        .items_center()
        .border_b_1()
        .border_color(palette.border.opacity(0.55))
        .child(
            div()
                .w(px(54.))
                .flex_shrink_0()
                .text_size(px(11.))
                .font_family(data_font_family())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(format!("G{:02}", channel.prn)),
        )
        .child(
            div()
                .w(px(110.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(div().size(px(7.)).rounded(px(2.)).bg(state_color))
                .child(
                    div()
                        .text_size(px(9.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(state_color)
                        .child(state),
                ),
        )
        .child(table_cell(
            format!("{:.1}", channel.cn0_db_hz),
            96.0,
            palette,
            false,
        ))
        .child(table_cell(
            format!("{:+.0}", channel.doppler_hz),
            104.0,
            palette,
            false,
        ))
        .child(table_cell(
            channel
                .residual_m
                .map(|residual| format!("{residual:+.3}"))
                .unwrap_or_else(|| "--".to_string()),
            104.0,
            palette,
            channel.residual_m.is_none(),
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
    let base_channels = crate::state::sample_channel_statuses();
    let epochs = (0..180)
        .map(|index| {
            let phase = index as f64 * 0.055;
            let mut epoch = base_epoch.clone();
            epoch.epoch_index = index;
            epoch.latitude_deg += phase.sin() * 0.000_012;
            epoch.longitude_deg += (phase * 0.7).cos() * 0.000_018;
            epoch.altitude_m += (phase * 0.45).sin() * 1.8;
            if let Some(velocity) = &mut epoch.velocity {
                velocity.speed_mps = 2.6 + (phase * 0.8).sin() * 0.25;
                velocity.horizontal_speed_mps = velocity.speed_mps;
            }
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
                    channel.cn0_db_hz += phase.sin() * 1.8 + (phase * 0.31).cos() * 0.7;
                    channel.doppler_hz += (phase * 0.2).sin() * 18.0;
                    channel
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    (epochs, channels)
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
            6_200,
            Some(1),
            GuiDiagnosticEventKind::Navigation,
            "Subframe synchronization established",
        ),
        (
            8_300,
            Some(11),
            GuiDiagnosticEventKind::ChannelState,
            "TrackingLocked -> Lost",
        ),
        (
            9_300,
            Some(11),
            GuiDiagnosticEventKind::Reacquisition,
            "Doppler +5400 Hz · metric 4.31",
        ),
        (
            10_300,
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
                .font_weight(gpui::FontWeight::SEMIBOLD)
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
        .font_weight(gpui::FontWeight::SEMIBOLD)
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

#[derive(Clone, Copy)]
struct SkyMarker {
    prn: u8,
    x: f32,
    y: f32,
    color: Hsla,
    azimuth_deg: f64,
    elevation_deg: f64,
    residual_m: f64,
    cn0_db_hz: Option<f64>,
    used_in_solution: bool,
}

fn sky_plot(
    satellites: &[GuiSatelliteGeometry],
    channels: &[GuiChannelStatus],
    hovered_prn: Option<u8>,
    pinned_prn: Option<u8>,
    palette: Palette,
    cx: &mut Context<GnsscopeApp>,
) -> impl IntoElement {
    let markers = satellites
        .iter()
        .filter_map(|satellite| {
            let (x, y) = sky_plot_position(satellite.azimuth_deg, satellite.elevation_deg)?;
            let channel = channels.iter().find(|channel| channel.prn == satellite.prn);
            let marker_color = if satellite.raim_excluded {
                palette.danger
            } else if channel.is_some_and(|channel| channel.tracking_locked)
                && satellite.used_in_solution
            {
                palette.healthy
            } else if channel.is_some() {
                palette.warning
            } else {
                palette.muted
            };
            Some(SkyMarker {
                prn: satellite.prn,
                x,
                y,
                color: marker_color,
                azimuth_deg: satellite.azimuth_deg,
                elevation_deg: satellite.elevation_deg,
                residual_m: satellite.residual_m,
                cn0_db_hz: channel.map(|channel| channel.cn0_db_hz),
                used_in_solution: satellite.used_in_solution,
            })
        })
        .collect::<Vec<_>>();
    let geometry_available = !markers.is_empty();
    let active_prn = hovered_prn.or(pinned_prn);
    let active_marker = active_prn.and_then(|prn| markers.iter().find(|marker| marker.prn == prn));
    let detail = active_marker.map(|marker| {
        format!(
            "PRN {:02}  ·  C/N0 {}  ·  方位 {:.1}°  ·  仰角 {:.1}°  ·  残差 {:+.2} m{}",
            marker.prn,
            marker
                .cn0_db_hz
                .map(|value| format!("{value:.1} dB-Hz"))
                .unwrap_or_else(|| "--".to_string()),
            marker.azimuth_deg,
            marker.elevation_deg,
            marker.residual_m,
            if marker.used_in_solution {
                "  ·  PVT"
            } else {
                ""
            },
        )
    });

    div()
        .size_full()
        .relative()
        .flex()
        .justify_center()
        .items_center()
        .child(
            div()
                .relative()
                .w(px(184.))
                .h(px(184.))
                .child(
                    canvas(
                        move |_, _, _| {},
                        move |plot_bounds, _, window, _| {
                            let diameter = plot_bounds.size.width.min(plot_bounds.size.height)
                                * SKY_PLOT_DIAMETER_FRACTION;
                            let origin = point(
                                plot_bounds.origin.x + (plot_bounds.size.width - diameter) / 2.,
                                plot_bounds.origin.y + (plot_bounds.size.height - diameter) / 2.,
                            );
                            for ratio in [1.0_f32, 2.0 / 3.0, 1.0 / 3.0] {
                                let ring_diameter = diameter * ratio;
                                let ring = Bounds {
                                    origin: point(
                                        origin.x + (diameter - ring_diameter) / 2.,
                                        origin.y + (diameter - ring_diameter) / 2.,
                                    ),
                                    size: size(ring_diameter, ring_diameter),
                                };
                                window.paint_quad(quad(
                                    ring,
                                    ring_diameter / 2.,
                                    palette.surface_alt,
                                    px(1.),
                                    palette.border,
                                    Default::default(),
                                ));
                            }
                            let center_x = origin.x + diameter / 2.;
                            let center_y = origin.y + diameter / 2.;
                            window.paint_quad(quad(
                                bounds(point(center_x - px(0.5), origin.y), size(px(1.), diameter)),
                                px(0.),
                                palette.border,
                                px(0.),
                                palette.border,
                                Default::default(),
                            ));
                            window.paint_quad(quad(
                                bounds(point(origin.x, center_y - px(0.5)), size(diameter, px(1.))),
                                px(0.),
                                palette.border,
                                px(0.),
                                palette.border,
                                Default::default(),
                            ));
                        },
                    )
                    .size_full(),
                )
                .children(markers.into_iter().map(move |marker| {
                    let prn = marker.prn;
                    let active = active_prn == Some(prn);
                    div()
                        .id(SharedString::from(format!("sky-prn-{prn}")))
                        .absolute()
                        .left(gpui::relative(marker.x))
                        .top(gpui::relative(marker.y))
                        .ml(px(-11.))
                        .mt(px(-11.))
                        .w(if active { px(26.) } else { px(22.) })
                        .h(if active { px(26.) } else { px(22.) })
                        .rounded(if active { px(13.) } else { px(11.) })
                        .bg(if active {
                            palette.accent
                        } else {
                            palette.surface
                        })
                        .border_2()
                        .border_color(if active { palette.accent } else { marker.color })
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(8.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if active {
                            palette.background
                        } else {
                            palette.text
                        })
                        .cursor_pointer()
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            if *hovered {
                                this.hovered_prn = Some(prn);
                            } else if this.hovered_prn == Some(prn) {
                                this.hovered_prn = None;
                            }
                            cx.notify();
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pinned_prn = (this.pinned_prn != Some(prn)).then_some(prn);
                            cx.notify();
                        }))
                        .child(format!("{:02}", marker.prn))
                }))
                .child(cardinal_label("N", 0.5, 0.0, palette))
                .child(cardinal_label("E", 1.0, 0.5, palette))
                .child(cardinal_label("S", 0.5, 1.0, palette))
                .child(cardinal_label("W", 0.0, 0.5, palette)),
        )
        .children((!geometry_available).then(|| {
            div()
                .absolute()
                .bottom(px(4.))
                .w_full()
                .text_center()
                .text_size(px(10.))
                .text_color(palette.muted)
                .child("当前 epoch 无卫星方位角/仰角")
        }))
        .children(detail.map(|detail| {
            div()
                .absolute()
                .bottom(px(0.))
                .left_0()
                .right_0()
                .text_center()
                .text_size(px(9.))
                .text_color(palette.text)
                .child(detail)
        }))
}

const SKY_PLOT_RADIUS_FRACTION: f32 = 0.43;
const SKY_PLOT_DIAMETER_FRACTION: f32 = SKY_PLOT_RADIUS_FRACTION * 2.0;

fn sky_plot_position(azimuth_deg: f64, elevation_deg: f64) -> Option<(f32, f32)> {
    if !azimuth_deg.is_finite()
        || !elevation_deg.is_finite()
        || !(0.0..=90.0).contains(&elevation_deg)
    {
        return None;
    }
    let radial_fraction = ((90.0 - elevation_deg) / 90.0) as f32;
    let azimuth_rad = azimuth_deg.to_radians();
    Some((
        0.5 + SKY_PLOT_RADIUS_FRACTION * radial_fraction * azimuth_rad.sin() as f32,
        0.5 - SKY_PLOT_RADIUS_FRACTION * radial_fraction * azimuth_rad.cos() as f32,
    ))
}

fn cardinal_label(label: &'static str, x: f32, y: f32, palette: Palette) -> impl IntoElement {
    div()
        .absolute()
        .left(gpui::relative(x))
        .top(gpui::relative(y))
        .ml(px(-7.))
        .mt(px(-7.))
        .w(px(14.))
        .h(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(8.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(palette.muted)
        .child(label)
}

#[derive(Debug, Clone, PartialEq)]
struct Cn0Series {
    prn: u8,
    samples: Vec<Option<f64>>,
}

impl Cn0Series {
    fn value_at(&self, selected_epoch: Option<usize>) -> Option<f64> {
        selected_epoch
            .and_then(|index| self.samples.get(index).copied().flatten())
            .or_else(|| self.samples.iter().rev().flatten().copied().next())
    }
}

fn build_cn0_series(
    epoch_channels: &[Vec<GuiChannelStatus>],
    selected_epoch: Option<usize>,
    pinned_prn: Option<u8>,
    limit: usize,
) -> Vec<Cn0Series> {
    let prns = epoch_channels
        .iter()
        .flat_map(|channels| channels.iter())
        .filter(|channel| channel.tracking_locked && channel.cn0_db_hz.is_finite())
        .map(|channel| channel.prn)
        .collect::<BTreeSet<_>>();
    let mut series = prns
        .into_iter()
        .map(|prn| Cn0Series {
            prn,
            samples: epoch_channels
                .iter()
                .map(|channels| {
                    channels
                        .iter()
                        .find(|channel| channel.prn == prn && channel.tracking_locked)
                        .map(|channel| channel.cn0_db_hz)
                        .filter(|value| value.is_finite())
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    series.sort_by(|left, right| {
        let left_pinned = Some(left.prn) == pinned_prn;
        let right_pinned = Some(right.prn) == pinned_prn;
        right_pinned.cmp(&left_pinned).then_with(|| {
            right
                .value_at(selected_epoch)
                .unwrap_or(f64::NEG_INFINITY)
                .total_cmp(&left.value_at(selected_epoch).unwrap_or(f64::NEG_INFINITY))
        })
    });
    series.truncate(limit);
    series
}

fn cn0_plot(
    epoch_channels: &[Vec<GuiChannelStatus>],
    current_channels: &[GuiChannelStatus],
    selected_epoch: Option<usize>,
    hovered_prn: Option<u8>,
    pinned_prn: Option<u8>,
    palette: Palette,
    cx: &mut Context<GnsscopeApp>,
) -> impl IntoElement {
    let fallback_history;
    let epoch_channels = if epoch_channels.is_empty() && !current_channels.is_empty() {
        fallback_history = vec![current_channels.to_vec()];
        fallback_history.as_slice()
    } else {
        epoch_channels
    };
    let series = build_cn0_series(epoch_channels, selected_epoch, pinned_prn, 8);
    let series_empty = series.is_empty();
    let sample_count = epoch_channels.len().max(1);
    let row_count = series.len().max(1);
    let plot_series = series.clone();
    let colors = palette.chart_series;
    div()
        .size_full()
        .relative()
        .child(
            canvas(
                move |_, _, _| {},
                move |plot_bounds, _, window, _| {
                    let left = plot_bounds.origin.x + px(42.);
                    let right = plot_bounds.origin.x + plot_bounds.size.width - px(44.);
                    let width = (right - left).max(px(1.));
                    let row_height = plot_bounds.size.height / row_count as f32;
                    for grid_index in 0..=4 {
                        let x = left + width * (grid_index as f32 / 4.0);
                        window.paint_quad(quad(
                            bounds(
                                point(x, plot_bounds.origin.y),
                                size(px(1.), plot_bounds.size.height),
                            ),
                            px(0.),
                            palette.border.opacity(0.55),
                            px(0.),
                            palette.border,
                            Default::default(),
                        ));
                    }
                    for row in 0..=row_count {
                        let y = plot_bounds.origin.y + row_height * row as f32;
                        window.paint_quad(quad(
                            bounds(point(left, y), size(width, px(1.))),
                            px(0.),
                            palette.border.opacity(0.75),
                            px(0.),
                            palette.border,
                            Default::default(),
                        ));
                    }
                    if let Some(index) = selected_epoch.filter(|index| *index < sample_count) {
                        let ratio = if sample_count <= 1 {
                            1.0
                        } else {
                            index as f32 / (sample_count - 1) as f32
                        };
                        let x = left + width * ratio;
                        window.paint_quad(quad(
                            bounds(
                                point(x, plot_bounds.origin.y),
                                size(px(1.), plot_bounds.size.height),
                            ),
                            px(0.),
                            palette.accent.opacity(0.85),
                            px(0.),
                            palette.accent,
                            Default::default(),
                        ));
                    }
                    for (row, item) in plot_series.iter().enumerate() {
                        let row_top = plot_bounds.origin.y + row_height * row as f32;
                        let inner_height = (row_height - px(6.)).max(px(1.));
                        let color = colors[row % colors.len()];
                        let available = item.samples.iter().flatten().copied().collect::<Vec<_>>();
                        let data_min = available.iter().copied().fold(f64::INFINITY, f64::min);
                        let data_max = available.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                        let center = (data_min + data_max) * 0.5;
                        let span = (data_max - data_min).max(6.0);
                        let min_y = center - span * 0.5;
                        let max_y = center + span * 0.5;
                        let mut builder = PathBuilder::stroke(px(1.2));
                        let mut segment_started = false;
                        let mut selected_point = None;
                        for (index, sample) in item.samples.iter().enumerate() {
                            let Some(value) = sample else {
                                segment_started = false;
                                continue;
                            };
                            let x_ratio = if sample_count <= 1 {
                                1.0
                            } else {
                                index as f32 / (sample_count - 1) as f32
                            };
                            let y_ratio = ((value.clamp(min_y, max_y) - min_y) / span) as f32;
                            let plot_point = point(
                                left + width * x_ratio,
                                row_top + px(3.) + inner_height * (1.0 - y_ratio),
                            );
                            if segment_started {
                                builder.line_to(plot_point);
                            } else {
                                builder.move_to(plot_point);
                                segment_started = true;
                            }
                            if selected_epoch == Some(index) || selected_epoch.is_none() {
                                selected_point = Some(plot_point);
                            }
                        }
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, color);
                        }
                        if let Some(point) = selected_point {
                            window.paint_quad(quad(
                                bounds(
                                    gpui::point(point.x - px(2.5), point.y - px(2.5)),
                                    size(px(5.), px(5.)),
                                ),
                                px(2.5),
                                color,
                                px(1.),
                                palette.surface,
                                Default::default(),
                            ));
                        }
                    }
                },
            )
            .size_full(),
        )
        .children(series.into_iter().enumerate().map(|(row, item)| {
            let prn = item.prn;
            let active = hovered_prn.or(pinned_prn) == Some(prn);
            let value = item.value_at(selected_epoch);
            let color = palette.chart_series[row % palette.chart_series.len()];
            div()
                .id(SharedString::from(format!("cn0-line-prn-{prn}")))
                .absolute()
                .left_0()
                .right_0()
                .top(gpui::relative(row as f32 / row_count as f32))
                .h(gpui::relative(1.0 / row_count as f32))
                .flex()
                .items_center()
                .justify_between()
                .px(px(4.))
                .cursor_pointer()
                .when(active, |this| this.bg(palette.accent.opacity(0.06)))
                .on_hover(cx.listener(move |app, hovered, _, cx| {
                    if *hovered {
                        app.hovered_prn = Some(prn);
                    } else if app.hovered_prn == Some(prn) {
                        app.hovered_prn = None;
                    }
                    cx.notify();
                }))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.pinned_prn = (app.pinned_prn != Some(prn)).then_some(prn);
                    cx.notify();
                }))
                .child(
                    div()
                        .w(px(34.))
                        .text_size(px(8.))
                        .font_family(data_font_family())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if active { palette.accent } else { color })
                        .child(format!("G{prn:02}")),
                )
                .child(
                    div()
                        .w(px(38.))
                        .text_right()
                        .text_size(px(8.))
                        .font_family(data_font_family())
                        .text_color(if active { palette.accent } else { palette.text })
                        .child(
                            value
                                .map(|value| format!("{value:.1}"))
                                .unwrap_or_else(|| "--".to_string()),
                        ),
                )
        }))
        .children(series_empty.then(|| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .text_color(palette.muted)
                .child("等待可用的通道 C/N0 历史")
        }))
}

fn pvt_position_is_displayable(pvt: &GuiPvtViewModel) -> bool {
    let quality = gnss_core::pvt::PositionQualityConfig::default();
    pvt.quality.converged
        && pvt.ecef_m.iter().all(|value| value.is_finite())
        && pvt.latitude_deg.is_finite()
        && pvt.longitude_deg.is_finite()
        && pvt.altitude_m.is_finite()
        && (quality.min_altitude_m..=quality.max_altitude_m).contains(&pvt.altitude_m)
}

fn position_plot(
    epochs: &[GuiPvtViewModel],
    selected_epoch: Option<usize>,
    palette: Palette,
) -> impl IntoElement {
    let lla = epochs
        .iter()
        .enumerate()
        .filter(|(_, pvt)| pvt_position_is_displayable(pvt))
        .map(|(index, pvt)| (index, pvt.latitude_deg, pvt.longitude_deg))
        .collect::<Vec<_>>();
    let points = lla
        .first()
        .map_or_else(Vec::new, |(_, latitude_0, longitude_0)| {
            let cos_latitude = latitude_0.to_radians().cos();
            lla.iter()
                .map(|(index, latitude, longitude)| {
                    let east = (longitude - longitude_0).to_radians() * 6_378_137.0 * cos_latitude;
                    let north = (latitude - latitude_0).to_radians() * 6_378_137.0;
                    (*index, east, north)
                })
                .collect()
        });
    canvas(
        move |_, _, _| {},
        move |plot_bounds, _, window, _| {
            let center = point(
                plot_bounds.origin.x + plot_bounds.size.width / 2.,
                plot_bounds.origin.y + plot_bounds.size.height / 2.,
            );
            window.paint_quad(quad(
                bounds(
                    point(center.x, plot_bounds.origin.y),
                    size(px(1.), plot_bounds.size.height),
                ),
                px(0.),
                palette.border,
                px(0.),
                palette.border,
                Default::default(),
            ));
            window.paint_quad(quad(
                bounds(
                    point(plot_bounds.origin.x, center.y),
                    size(plot_bounds.size.width, px(1.)),
                ),
                px(0.),
                palette.border,
                px(0.),
                palette.border,
                Default::default(),
            ));
            if points.is_empty() {
                return;
            }
            let mut extents = points
                .iter()
                .map(|(_, east, north)| east.abs().max(north.abs()))
                .filter(|extent| extent.is_finite())
                .collect::<Vec<_>>();
            extents.sort_by(f64::total_cmp);
            let robust_index = ((extents.len() - 1) as f64 * 0.95).round() as usize;
            let extent = extents[robust_index].max(1.0);
            let half_width = plot_bounds.size.width / 2. - px(8.);
            let half_height = plot_bounds.size.height / 2. - px(8.);
            for (index, east, north) in &points {
                if east.abs() > extent || north.abs() > extent {
                    continue;
                }
                let x = center.x + half_width * (*east / extent) as f32;
                let y = center.y - half_height * (*north / extent) as f32;
                let selected = Some(*index) == selected_epoch;
                let diameter = if selected { px(6.) } else { px(3.) };
                window.paint_quad(quad(
                    bounds(
                        point(x - diameter / 2., y - diameter / 2.),
                        size(diameter, diameter),
                    ),
                    diameter / 2.,
                    if selected {
                        palette.accent
                    } else {
                        palette.healthy
                    },
                    px(0.),
                    palette.accent,
                    Default::default(),
                ));
            }
        },
    )
    .size_full()
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

fn altitude_statistics(epochs: &[GuiPvtViewModel]) -> String {
    let points = epochs
        .iter()
        .enumerate()
        .filter(|(_, pvt)| pvt_position_is_displayable(pvt))
        .map(|(index, pvt)| (index as f64, pvt.altitude_m))
        .collect::<Vec<_>>();
    let values = points.iter().map(|point| point.1).collect::<Vec<_>>();
    if values.is_empty() {
        return "等待有效高度".to_string();
    }
    let (mean, standard_deviation) = mean_and_population_sigma(&values);
    let averaged = moving_average_points(&points, 10)
        .into_iter()
        .map(|point| point.1)
        .collect::<Vec<_>>();
    let averaged_sigma = (!averaged.is_empty()).then(|| mean_and_population_sigma(&averaged).1);
    let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    match averaged_sigma {
        Some(averaged_sigma) => format!(
            "均值 {:.1} m · 原始 σ {:.1} m · 10 点 σ {:.1} m · {:.1}–{:.1} m",
            mean, standard_deviation, averaged_sigma, minimum, maximum
        ),
        None => format!(
            "均值 {:.1} m · 原始 σ {:.1} m · {:.1}–{:.1} m",
            mean, standard_deviation, minimum, maximum
        ),
    }
}

fn mean_and_population_sigma(values: &[f64]) -> (f64, f64) {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let sigma = (values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64)
        .sqrt();
    (mean, sigma)
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
mod sky_plot_tests {
    use super::sky_plot_position;

    #[test]
    fn sky_plot_uses_north_up_and_east_right() {
        let north_horizon = sky_plot_position(0.0, 0.0).unwrap();
        let east_horizon = sky_plot_position(90.0, 0.0).unwrap();
        let zenith = sky_plot_position(225.0, 90.0).unwrap();

        assert!((north_horizon.0 - 0.5).abs() < 1e-6);
        assert!(north_horizon.1 < 0.5);
        assert!(east_horizon.0 > 0.5);
        assert!((east_horizon.1 - 0.5).abs() < 1e-6);
        assert!((zenith.0 - 0.5).abs() < 1e-6);
        assert!((zenith.1 - 0.5).abs() < 1e-6);
        assert!(sky_plot_position(0.0, -0.1).is_none());
    }
}

#[cfg(test)]
mod position_quality_tests {
    use super::{altitude_statistics, moving_average_points, pvt_position_is_displayable};

    #[test]
    fn legacy_implausible_altitude_is_excluded_from_position_and_statistics() {
        let mut valid = crate::sample_pvt_view_model();
        valid.altitude_m = 70.0;
        let mut implausible = valid.clone();
        implausible.altitude_m = 1_123_486.0;
        let mut stratospheric = valid.clone();
        stratospheric.altitude_m = 90_225.0;

        assert!(pvt_position_is_displayable(&valid));
        assert!(!pvt_position_is_displayable(&implausible));
        assert!(!pvt_position_is_displayable(&stratospheric));
        assert_eq!(
            altitude_statistics(&[valid, implausible]),
            "均值 70.0 m · 原始 σ 0.0 m · 70.0–70.0 m"
        );
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
}

#[cfg(test)]
mod tests {
    use super::{
        build_cn0_series, epoch_index_for_percent, preview_diagnostic_events,
        preview_epoch_history, toggled_side_drawer, InterfaceLanguage, PlotFocus, SettingsCategory,
        SideDrawer, ThemePreference, WorkspaceView,
    };

    #[test]
    fn timeline_percent_maps_to_shared_epoch_index() {
        assert_eq!(epoch_index_for_percent(0, 50.0), None);
        assert_eq!(epoch_index_for_percent(1, 0.0), Some(0));
        assert_eq!(epoch_index_for_percent(101, 0.0), Some(0));
        assert_eq!(epoch_index_for_percent(101, 50.0), Some(50));
        assert_eq!(epoch_index_for_percent(101, 100.0), Some(100));
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

        assert_eq!(events.len(), 5);
        assert_eq!(events.first().and_then(|event| event.prn), Some(1));
        assert_eq!(events.last().and_then(|event| event.prn), Some(24));
        assert!(events
            .windows(2)
            .all(|pair| pair[0].now_ms < pair[1].now_ms));
    }

    #[test]
    fn visual_preview_provides_aligned_cn0_history() {
        let (epochs, channels) = preview_epoch_history();
        let series = build_cn0_series(&channels, epochs.len().checked_sub(1), Some(20), 8);

        assert_eq!(epochs.len(), 180);
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
