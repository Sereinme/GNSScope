use crate::{
    spawn_streaming_worker, ActiveWorker, GuiChannelStatus, GuiDiagnosticEvent,
    GuiProcessingSettings, GuiPvtViewModel, GuiSatelliteGeometry, GuiSettingsIssue, ProcessingRun,
    RunLibrary, RunStatus, StoredEpoch, WorkerEvent,
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
    ActiveTheme, Disableable, IconName, Theme, ThemeMode, TitleBar,
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

fn color(value: u32) -> Hsla {
    rgb(value).into()
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
        .split(|character: char| character == ',' || character == ';')
        .map(|part| part.trim().parse::<f64>().map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()?;
    (values.len() == 3)
        .then(|| [values[0], values[1], values[2]])
        .ok_or(())
        .map(Some)
}

#[derive(Clone, Copy)]
enum ThemePreference {
    System,
    Light,
    Dark,
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

    fn label(self) -> &'static str {
        match self {
            Self::Input => "输入",
            Self::Acquisition => "捕获",
            Self::Tracking => "跟踪",
            Self::Pvt => "PVT",
            Self::Performance => "性能",
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

    fn description(self) -> &'static str {
        match self {
            Self::Input => "采样、IQ 与导航来源",
            Self::Acquisition => "冷启动与周期性重捕",
            Self::Tracking => "环路、锁定与信号质量",
            Self::Pvt => "输出、滤波与完整性",
            Self::Performance => "并行处理策略",
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

    fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::System => "系统",
            Self::Light => "浅色",
            Self::Dark => "深色",
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
}

impl Palette {
    fn light() -> Self {
        Self {
            background: color(0xf1f4f5),
            sidebar: color(0xe5ebed),
            surface: color(0xfbfcfc),
            surface_alt: color(0xf3f7f7),
            border: color(0xcdd8dc),
            text: color(0x152027),
            muted: color(0x667780),
            accent: color(0x147d99),
            healthy: color(0x137653),
            warning: color(0xb46e12),
            danger: color(0xb9363e),
        }
    }

    fn dark() -> Self {
        Self {
            background: color(0x0f1417),
            sidebar: color(0x151b1f),
            surface: color(0x1a2126),
            surface_alt: color(0x202930),
            border: color(0x303c44),
            text: color(0xecf2f4),
            muted: color(0x91a0a9),
            accent: color(0x4bb6d0),
            healthy: color(0x4db98d),
            warning: color(0xe0a044),
            danger: color(0xe56a6a),
        }
    }
}

pub struct GnsscopeApp {
    settings: GuiProcessingSettings,
    library: Option<RunLibrary>,
    history: Vec<ProcessingRun>,
    queue: VecDeque<ProcessingRun>,
    active_run: Option<ProcessingRun>,
    live_run_id: Option<Uuid>,
    selected_run_id: Option<Uuid>,
    worker: Option<ActiveWorker>,
    worker_receiver: Option<Receiver<WorkerEvent>>,
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
    system_dark: bool,
    side_drawer: Option<SideDrawer>,
    settings_category: SettingsCategory,
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
        match theme_preference {
            ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
            ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
        let timeline_visible = library
            .as_ref()
            .and_then(|library| library.preference("timeline_visible").ok().flatten())
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
                .placeholder("搜索文件名或路径")
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
        let mut app = Self {
            settings,
            library,
            history,
            queue,
            active_run: None,
            live_run_id: None,
            selected_run_id: initial_run_id,
            worker: None,
            worker_receiver: None,
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
            timeline_visible,
            status_message: "请选择 IQ 文件开始解算".to_string(),
        };
        if std::env::var("GNSSCOPE_PREVIEW")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        {
            app.history.clear();
            app.queue.clear();
            app.selected_run_id = None;
            app.live_run_id = None;
            app.historical_epochs = vec![crate::sample_pvt_view_model()];
            app.historical_channels = vec![crate::state::sample_channel_statuses()];
            app.selected_epoch = Some(0);
            app.status_message = "确定性界面预览".to_string();
        }
        if std::env::var_os("GNSSCOPE_AUTORUN").is_some() {
            app.enqueue();
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

    fn cycle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_preference = self.theme_preference.next();
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

    fn toggle_timeline_visibility(&mut self) {
        self.timeline_visible = !self.timeline_visible;
        if let Some(library) = &self.library {
            let _ = library.set_preference(
                "timeline_visible",
                if self.timeline_visible {
                    "true"
                } else {
                    "false"
                },
            );
        }
    }

    fn schedule_poll(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(50)).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.drain_worker_events();
                    this.schedule_poll(cx);
                    cx.notify();
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
                        this.settings.iq_file_path = Some(path);
                        this.selected_run_id = None;
                        this.historical_epochs.clear();
                        this.historical_channels.clear();
                        this.historical_diagnostic_events.clear();
                        this.selected_epoch = None;
                        this.status_message = "IQ 文件已就绪".to_string();
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn apply_cttc_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.settings.iq_file_path.clone() else {
            self.status_message = "请先选择 CTTC IQ 文件".to_string();
            return;
        };
        self.settings = GuiProcessingSettings::cttc_spain(path);
        self.sync_setting_inputs(window, cx);
        self.status_message = "已应用 CTTC Spain 4 Msps / Int16 预设".to_string();
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
            self.setting_parse_errors
                .insert(field, "请输入有效数值或规定格式");
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

    fn edit_selected_configuration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.selected_run().map(|run| run.settings.clone()) else {
            return;
        };
        self.settings = settings;
        self.selected_run_id = None;
        self.selected_epoch = None;
        self.follow_live = false;
        self.sync_setting_inputs(window, cx);
        self.status_message = "历史配置已复制为可编辑草稿".to_string();
    }

    fn cycle_iq_format(&mut self) {
        if self.selected_run_id.is_some() || self.active_run.is_some() {
            return;
        }
        self.settings.iq_format = match self.settings.iq_format {
            crate::GuiIqFormat::Int8Interleaved => crate::GuiIqFormat::Int16Interleaved,
            crate::GuiIqFormat::Int16Interleaved => crate::GuiIqFormat::Int8Interleaved,
        };
    }

    fn toggle_setting(&mut self, field: &'static str) {
        if self.selected_run_id.is_some() || self.active_run.is_some() {
            return;
        }
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

    fn enqueue(&mut self) {
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
                }
            }
            Err(error) => self.status_message = format!("无法创建运行记录：{error}"),
        }
    }

    fn enqueue_selected_or_draft(&mut self) {
        if let Some(settings) = self.selected_run().map(|run| run.settings.clone()) {
            self.settings = settings;
            self.setting_parse_errors.clear();
        }
        self.enqueue();
    }

    fn start_run(&mut self, mut run: ProcessingRun) {
        let (sender, receiver) = mpsc::channel();
        let worker = spawn_streaming_worker(run.settings.clone(), sender);
        run.status = RunStatus::Processing;
        run.message = "初始化接收机".to_string();
        if let Some(library) = &self.library {
            let _ = library.update_progress(run.id, RunStatus::Processing, 0, 0, 0, &run.message);
        }
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

    fn drain_worker_events(&mut self) {
        let events = self
            .worker_receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            self.apply_worker_event(event);
        }
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
                    if let Some(library) = &self.library {
                        let _ = library.update_progress(
                            run.id,
                            RunStatus::Processing,
                            processed_samples,
                            total_samples,
                            self.epochs.len(),
                            &phase,
                        );
                    }
                }
                self.status_message = phase;
            }
            WorkerEvent::Epoch { pvt, channels } => {
                let epoch_ordinal = self.epochs.len();
                if let Some(library) = &self.library {
                    let _ = library.append_epoch(run_id, epoch_ordinal, &pvt, &channels);
                }
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
                if let Some(library) = &self.library {
                    let _ = library.append_event(run_id, self.diagnostic_events.len(), &event);
                }
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
            if let Some(library) = &self.library {
                let _ = library.update_progress(
                    run.id,
                    status,
                    processed,
                    total,
                    self.epochs.len(),
                    message,
                );
            }
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

    fn select_run(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
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

    fn selected_run(&self) -> Option<&ProcessingRun> {
        self.selected_run_id.and_then(|id| {
            self.active_run
                .as_ref()
                .filter(|run| run.id == id)
                .or_else(|| self.history.iter().find(|run| run.id == id))
        })
    }

    fn display_settings(&self) -> &GuiProcessingSettings {
        self.selected_run()
            .map(|run| &run.settings)
            .unwrap_or(&self.settings)
    }

    fn settings_issues(&self) -> Vec<GuiSettingsIssue> {
        crate::state::validate_settings(&self.display_settings().normalized_for_processing())
    }

    fn can_enqueue(&self) -> bool {
        self.active_run.is_none()
            && self.setting_parse_errors.is_empty()
            && self.settings_issues().is_empty()
    }

    fn file_label(&self) -> SharedString {
        self.selected_run()
            .map(|run| &run.source_path)
            .or(self.settings.iq_file_path.as_ref())
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未选择数据文件".to_string())
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
            .items_end()
            .justify_between()
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(palette.text)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(subtitle),
            )
    }

    fn metric(
        label: &'static str,
        value: String,
        unit: &'static str,
        palette: Palette,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_w(px(120.))
            .py(px(10.))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(palette.muted)
                    .child(label),
            )
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .items_baseline()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(value),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .child(unit),
                    ),
            )
    }

    fn render_title_bar(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = self.active_run.is_some();
        let settings = self.display_settings();
        let source_summary = format!(
            "GPS L1 C/A  ·  {:.3} Msps  ·  IF {:.3} MHz  ·  {:?}",
            settings.sample_rate_hz / 1e6,
            settings.intermediate_frequency_hz / 1e6,
            settings.iq_format
        );

        TitleBar::new()
            .bg(palette.sidebar)
            .border_color(palette.border)
            .text_color(palette.text)
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .child(IconName::Map)
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("GNSScope"),
                            ),
                    )
                    .child(
                        div()
                            .mx(px(3.))
                            .h(px(16.))
                            .w(px(1.))
                            .flex_shrink_0()
                            .bg(palette.border),
                    )
                    .child(
                        Button::new("file")
                            .ghost()
                            .text_color(palette.text)
                            .icon(IconName::FolderOpen)
                            .tooltip("选择 IQ 文件")
                            .on_click(cx.listener(|this, _, _, cx| this.select_file(cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(9.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(palette.text)
                            .child(self.file_label()),
                    )
                    .children((!compact).then(|| {
                        div()
                            .max_w(px(320.))
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(9.))
                            .text_color(palette.muted)
                            .child(source_summary)
                    }))
                    .child(
                        div()
                            .mx(px(3.))
                            .h(px(16.))
                            .w(px(1.))
                            .flex_shrink_0()
                            .bg(palette.border),
                    )
                    .children((is_active && !self.follow_live).then(|| {
                        Button::new("return-live")
                            .ghost()
                            .text_color(palette.accent)
                            .icon(IconName::Redo2)
                            .tooltip("返回最新 epoch")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.return_to_live(window, cx);
                                cx.notify();
                            }))
                    }))
                    .child(
                        Button::new("timeline-visibility")
                            .ghost()
                            .text_color(palette.text)
                            .icon(if self.timeline_visible {
                                IconName::PanelBottom
                            } else {
                                IconName::PanelBottomOpen
                            })
                            .tooltip(if self.timeline_visible {
                                "隐藏 epoch 时间轴"
                            } else {
                                "显示 epoch 时间轴"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_timeline_visibility();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings")
                            .ghost()
                            .text_color(palette.text)
                            .icon(IconName::Settings2)
                            .tooltip("接收机详细配置")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.side_drawer =
                                    toggled_side_drawer(this.side_drawer, SideDrawer::Settings);
                                cx.notify();
                            })),
                    )
                    .child(if is_active {
                        Button::new("cancel")
                            .danger()
                            .icon(IconName::CircleX)
                            .tooltip("取消当前解算")
                            .on_click(cx.listener(|this, _, _, _| this.cancel_active()))
                    } else if self.selected_run_id.is_some() {
                        Button::new("run")
                            .primary()
                            .icon(IconName::Replace)
                            .tooltip("按此配置重新解算")
                            .disabled(!self.can_enqueue())
                            .on_click(cx.listener(|this, _, _, _| this.enqueue_selected_or_draft()))
                    } else {
                        Button::new("run")
                            .primary()
                            .label("开始解算")
                            .disabled(!self.can_enqueue())
                            .on_click(cx.listener(|this, _, _, _| this.enqueue_selected_or_draft()))
                    }),
            )
    }

    fn render_sidebar(
        &self,
        palette: Palette,
        stacked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let history_query = self.history_search.read(cx).value().to_string();
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
                this.w(px(258.)).min_w(px(220.)).h_full().border_r_1()
            })
            .when(stacked, |this| {
                this.w_full().h(px(170.)).flex_shrink_0().border_b_1()
            })
            .bg(palette.sidebar)
            .border_color(palette.border)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(64.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(palette.text)
                                    .child("GNSScope"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(palette.muted)
                                    .child("GNSS SIGNAL LAB"),
                            ),
                    )
                    .child(
                        Button::new("theme")
                            .ghost()
                            .text_color(palette.text)
                            .icon(IconName::Palette)
                            .tooltip(format!("主题：{}", self.theme_preference.label()))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cycle_theme(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .p(px(12.))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .mb(px(7.))
                            .child("当前任务"),
                    )
                    .children(active)
                    .when(self.active_run.is_none(), |this| {
                        this.child(
                            div()
                                .p(px(10.))
                                .text_size(px(11.))
                                .text_color(palette.muted)
                                .child("暂无运行中的任务"),
                        )
                    })
                    .child(
                        div()
                            .mt(px(18.))
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .mb(px(7.))
                            .child(format!("等待队列  {}", self.queue.len())),
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
                                            .tooltip("上移")
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
                                            .tooltip("下移")
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
                                        .tooltip("取消排队")
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
                                    .child("历史记录"),
                            )
                            .child(
                                Button::new("history-filter")
                                    .ghost()
                                    .icon(IconName::SortDescending)
                                    .label(self.history_filter.label())
                                    .tooltip("切换历史状态过滤")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.history_filter = this.history_filter.next();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mb(px(8.))
                            .h(px(32.))
                            .child(Input::new(&self.history_search).prefix(IconName::Search)),
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
                                                .tooltip("在文件管理器中定位")
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
                                                .tooltip("删除历史记录")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.delete_historical_run(id);
                                                    cx.notify();
                                                })),
                                            ),
                                    )
                            }),
                    ),
            )
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
        div()
            .px(px(22.))
            .pt(px(18.))
            .pb(px(16.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(palette.muted)
                                    .child("处理状态"),
                            )
                            .child(
                                div()
                                    .mt(px(3.))
                                    .text_size(px(17.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(palette.text)
                                    .child(status_message),
                            ),
                    )
                    .child(
                        div()
                            .text_right()
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(palette.accent)
                                    .child(format!("{:.1}%", progress * 100.0)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(palette.muted)
                                    .child(format!("{elapsed:.1} / {total:.1} s")),
                            ),
                    ),
            )
            .child(
                Progress::new()
                    .mt(px(11.))
                    .h(px(7.))
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
            .child(div().mt(px(4.)).h(px(34.)).child(Input::new(input)))
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
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.display_settings();
        let issues = self.settings_issues();
        let editable = self.selected_run_id.is_none() && self.active_run.is_none();
        let category = self.settings_category;
        let issue_count = issues.len() + self.setting_parse_errors.len();
        div()
            .w(if compact { px(360.) } else { px(500.) })
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.surface)
            .border_l_1()
            .border_color(palette.border)
            .shadow_lg()
            .child(
                div()
                    .flex_shrink_0()
                    .p(px(18.))
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(17.))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(palette.text)
                                            .child("接收机配置"),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.))
                                            .text_size(px(10.))
                                            .text_color(palette.muted)
                                            .child(if editable {
                                                "更改将作为下一次解算的配置快照"
                                            } else {
                                                "历史任务快照 · 只读"
                                            }),
                                    ),
                            )
                            .child(
                                Button::new("close-settings-drawer")
                                    .ghost()
                                    .icon(IconName::CircleX)
                                    .tooltip("关闭接收机配置")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.side_drawer = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(14.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(11.))
                                    .text_color(palette.text)
                                    .child(IconName::Eye)
                                    .child("epoch 时间轴"),
                            )
                            .child(
                                Button::new("timeline-visibility-drawer")
                                    .ghost()
                                    .icon(if self.timeline_visible {
                                        IconName::Eye
                                    } else {
                                        IconName::EyeOff
                                    })
                                    .tooltip(if self.timeline_visible {
                                        "隐藏 epoch 时间轴"
                                    } else {
                                        "显示 epoch 时间轴"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_timeline_visibility();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children((editable || self.selected_run_id.is_some()).then(|| {
                        Button::new("cttc-preset")
                            .mt(px(12.))
                            .ghost()
                            .icon(IconName::Star)
                            .label(if editable {
                                "应用 CTTC Spain 预设"
                            } else {
                                "复制为可编辑草稿"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.selected_run_id.is_some() {
                                    this.edit_selected_configuration(window, cx);
                                } else {
                                    this.apply_cttc_preset(window, cx);
                                }
                                cx.notify();
                            }))
                    }))
                    .children((issue_count > 0).then(|| {
                        div()
                            .mt(px(12.))
                            .p(px(9.))
                            .rounded(px(5.))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .bg(palette.danger.opacity(0.10))
                            .border_1()
                            .border_color(palette.danger.opacity(0.45))
                            .text_size(px(10.))
                            .text_color(palette.danger)
                            .child(IconName::TriangleAlert)
                            .child(format!(
                                "{issue_count} 项配置需要处理{}",
                                issues
                                    .first()
                                    .map(|issue| format!(" · {}", issue.message))
                                    .unwrap_or_default()
                            ))
                    })),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(14.))
                    .py(px(10.))
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .border_b_1()
                    .border_color(palette.border)
                    .children(SettingsCategory::ALL.into_iter().map(|candidate| {
                        let selected = category == candidate;
                        Button::new(SharedString::from(format!(
                            "settings-category-{:?}",
                            candidate
                        )))
                        .ghost()
                        .icon(candidate.icon())
                        .label(candidate.label())
                        .text_color(if selected {
                            palette.accent
                        } else {
                            palette.muted
                        })
                        .when(selected, |button| {
                            button
                                .bg(palette.accent.opacity(0.12))
                                .border_1()
                                .border_color(palette.accent.opacity(0.35))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings_category = candidate;
                            cx.notify();
                        }))
                    })),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(18.))
                    .pt(px(14.))
                    .pb(px(8.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(category.label()),
                    )
                    .child(
                        div()
                            .mt(px(3.))
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .child(category.description()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px(px(18.))
                    .pb(px(20.))
                    .child(if editable {
                        self.render_settings_category_content(category, palette, cx)
                            .into_any_element()
                    } else {
                        self.render_read_only_category(category, settings, palette)
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(18.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(palette.border)
                    .text_size(px(9.))
                    .text_color(palette.muted)
                    .child("草稿即时验证 · 入队后配置冻结"),
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
            .child(Self::config_group_title("采样", palette))
            .child(self.render_setting_input("采样率", "sample_rate_hz", "Msps", palette))
            .child(self.render_setting_input("中频", "intermediate_frequency_hz", "MHz", palette))
            .child(self.render_setting_input("读取 chunk", "chunk_ms", "ms", palette))
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
                            .child("IQ 格式"),
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
            .child(Self::config_group_title("导航来源", palette))
            .child(self.render_setting_input(
                "RINEX 导航文件",
                "rinex_nav_path",
                "可选路径",
                palette,
            ))
            .child(self.render_setting_toggle(
                "允许环境变量导航文件",
                self.settings.use_rinex_nav_environment,
                "use_rinex_nav_environment",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                "自动发现同目录导航文件",
                self.settings.auto_discover_colocated_nav,
                "auto_discover_colocated_nav",
                palette,
                cx,
            ))
            .child(self.render_setting_input(
                "初始位置",
                "initial_position_lla_deg_m",
                "纬度, 经度, 高程",
                palette,
            ))
    }

    fn render_acquisition_settings(&self, palette: Palette) -> impl IntoElement {
        div()
            .child(Self::config_group_title("搜索窗口", palette))
            .child(self.render_setting_input("捕获窗口", "acquisition_window_ms", "ms", palette))
            .child(self.render_setting_input(
                "非相干累加",
                "acquisition_noncoherent_integrations",
                "次",
                palette,
            ))
            .child(self.render_setting_input("捕获阈值", "acquisition_threshold", "", palette))
            .child(self.render_setting_input(
                "最大多普勒",
                "acquisition_doppler_max_hz",
                "Hz",
                palette,
            ))
            .child(self.render_setting_input(
                "多普勒步进",
                "acquisition_doppler_step_hz",
                "Hz",
                palette,
            ))
            .child(Self::config_group_title("调度策略", palette))
            .child(self.render_setting_input(
                "冷启动扫描周期",
                "cold_acquisition_interval_ms",
                "ms",
                palette,
            ))
            .child(self.render_setting_input(
                "重捕获周期",
                "reacquisition_interval_ms",
                "ms",
                palette,
            ))
            .child(self.render_setting_input(
                "单次最大 PRN",
                "max_acquisition_prns_per_scan",
                "1-32",
                palette,
            ))
            .child(self.render_setting_input("固定 PRN", "fixed_prns", "逗号分隔", palette))
    }

    fn render_tracking_settings(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .child(Self::config_group_title("环路参数", palette))
            .child(self.render_setting_input("DLL 带宽", "dll_bandwidth_hz", "Hz", palette))
            .child(self.render_setting_input("PLL 带宽", "pll_bandwidth_hz", "Hz", palette))
            .child(self.render_setting_input(
                "早迟间距",
                "early_late_spacing_chips",
                "chips",
                palette,
            ))
            .child(self.render_setting_toggle(
                "载波辅助码环",
                self.settings.carrier_aiding,
                "carrier_aiding",
                palette,
                cx,
            ))
            .child(Self::config_group_title("锁定与准入", palette))
            .child(self.render_setting_input(
                "PLL lock 阈值",
                "minimum_pll_lock_metric",
                "0-1",
                palette,
            ))
            .child(self.render_setting_input("最低 C/N0", "minimum_cn0_db_hz", "dB-Hz", palette))
            .child(self.render_setting_input(
                "准入观察期",
                "probation_good_epochs",
                "epochs",
                palette,
            ))
            .child(Self::config_group_title("C/N0 估计", palette))
            .child(self.render_setting_input("C/N0 窗长", "cn0_samples", "samples", palette))
            .child(self.render_setting_input("C/N0 平滑系数", "cn0_smoother_alpha", "0-1", palette))
    }

    fn render_pvt_settings(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(Self::config_group_title("输出节奏", palette))
            .child(self.render_setting_input("PVT 输出 epoch", "pvt_output_rate_ms", "ms", palette))
            .child(self.render_setting_toggle(
                "epoch 多普勒平均",
                self.settings.average_doppler_over_pvt_epoch,
                "average_doppler_over_pvt_epoch",
                palette,
                cx,
            ))
            .child(Self::config_group_title("滤波与完整性", palette))
            .child(self.render_setting_toggle(
                "载波平滑",
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
                "整数毫秒模糊度解算",
                self.settings.enable_integer_ms_ambiguity_resolution,
                "enable_integer_ms_ambiguity_resolution",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                "RAIM 故障检测与排除",
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
            .child(Self::config_group_title("并行处理", palette))
            .child(self.render_setting_toggle(
                "并行捕获",
                self.settings.parallel_acquisition,
                "parallel_acquisition",
                palette,
                cx,
            ))
            .child(self.render_setting_toggle(
                "并行跟踪",
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
                    .child("并行处理只影响计算调度，不改变确定性的 PRN 合并顺序。"),
            )
    }

    fn render_read_only_category(
        &self,
        category: SettingsCategory,
        settings: &GuiProcessingSettings,
        palette: Palette,
    ) -> impl IntoElement {
        let rows: Vec<(&'static str, String)> = match category {
            SettingsCategory::Input => vec![
                (
                    "采样率",
                    format!("{:.3} Msps", settings.sample_rate_hz / 1e6),
                ),
                (
                    "中频",
                    format!("{:.3} MHz", settings.intermediate_frequency_hz / 1e6),
                ),
                ("IQ 格式", format!("{:?}", settings.iq_format)),
                (
                    "导航来源",
                    if settings.rinex_nav_path.is_some() {
                        "外部 RINEX"
                    } else {
                        "IQ-only"
                    }
                    .to_string(),
                ),
            ],
            SettingsCategory::Acquisition => vec![
                ("捕获窗口", format!("{} ms", settings.acquisition_window_ms)),
                (
                    "非相干累加",
                    format!("{} 次", settings.acquisition_noncoherent_integrations),
                ),
                (
                    "重捕获周期",
                    format!("{} ms", settings.reacquisition_interval_ms),
                ),
                (
                    "最大 PRN",
                    settings.max_acquisition_prns_per_scan.to_string(),
                ),
            ],
            SettingsCategory::Tracking => vec![
                (
                    "DLL / PLL",
                    format!(
                        "{:.1} / {:.1} Hz",
                        settings.dll_bandwidth_hz, settings.pll_bandwidth_hz
                    ),
                ),
                (
                    "最低 C/N0",
                    format!("{:.1} dB-Hz", settings.minimum_cn0_db_hz),
                ),
                (
                    "PLL lock",
                    format!("{:.2}", settings.minimum_pll_lock_metric),
                ),
            ],
            SettingsCategory::Pvt => vec![
                ("PVT epoch", format!("{} ms", settings.pvt_output_rate_ms)),
                (
                    "载波平滑",
                    if settings.enable_carrier_smoothing {
                        "开启"
                    } else {
                        "关闭"
                    }
                    .to_string(),
                ),
                (
                    "Kalman",
                    if settings.enable_kalman {
                        "开启"
                    } else {
                        "关闭"
                    }
                    .to_string(),
                ),
                (
                    "RAIM",
                    if settings.enable_raim {
                        "开启"
                    } else {
                        "关闭"
                    }
                    .to_string(),
                ),
            ],
            SettingsCategory::Performance => vec![
                (
                    "并行捕获",
                    if settings.parallel_acquisition {
                        "开启"
                    } else {
                        "关闭"
                    }
                    .to_string(),
                ),
                (
                    "并行跟踪",
                    if settings.parallel_tracking {
                        "开启"
                    } else {
                        "关闭"
                    }
                    .to_string(),
                ),
            ],
        };
        div()
            .rounded(px(6.))
            .border_1()
            .border_color(palette.border)
            .bg(palette.surface_alt)
            .p(px(12.))
            .children(rows.into_iter().map(|(label, value)| {
                div()
                    .py(px(9.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(palette.border.opacity(0.55))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(palette.text)
                            .child(value),
                    )
            }))
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
                Button::new(SharedString::from(format!("{field}-toggle")))
                    .ghost()
                    .icon(if enabled {
                        IconName::CircleCheck
                    } else {
                        IconName::Dash
                    })
                    .bg(if enabled {
                        palette.healthy.opacity(0.12)
                    } else {
                        palette.surface_alt
                    })
                    .border_1()
                    .border_color(if enabled {
                        palette.healthy.opacity(0.35)
                    } else {
                        palette.border
                    })
                    .text_color(if enabled {
                        palette.healthy
                    } else {
                        palette.muted
                    })
                    .label(if enabled { "开启" } else { "关闭" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_setting(field);
                        cx.notify();
                    })),
            )
    }

    fn render_overview(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let epochs = self.display_epochs();
        let channels = self.display_channels();
        let pvt = self.current_pvt();
        let valid_pvt = pvt.filter(|pvt| pvt_position_is_displayable(pvt));
        let satellites = valid_pvt
            .map(|pvt| pvt.satellites.as_slice())
            .unwrap_or(&[]);
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
                    "稳定通道 C/N0".to_string(),
                    "仅显示 Healthy PRN".to_string(),
                    cn0_plot(channels, self.hovered_prn, self.pinned_prn, palette, cx)
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
                .px(px(22.))
                .pb(px(30.))
                .child(self.plot_panel(title, subtitle, plot, palette, focus, true, cx))
                .into_any_element();
        }
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
            .px(px(22.))
            .pb(px(30.))
            .child(
                div()
                    .grid()
                    .grid_cols(if compact { 1 } else { 2 })
                    .gap(px(12.))
                    .child(self.plot_panel(
                        "卫星天空图",
                        "真实方位/仰角",
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
                        false,
                        cx,
                    ))
                    .child(self.plot_panel(
                        "ENU 位置",
                        "首个有效 epoch 为原点",
                        position_plot(epochs, self.selected_epoch, palette),
                        palette,
                        PlotFocus::Position,
                        false,
                        cx,
                    )),
            )
            .child(div().mt(px(12.)).child(self.plot_panel(
                "稳定通道 C/N0",
                "仅显示 Healthy PRN",
                cn0_plot(channels, self.hovered_prn, self.pinned_prn, palette, cx),
                palette,
                PlotFocus::Cn0,
                false,
                cx,
            )))
            .child(
                div()
                    .mt(px(12.))
                    .grid()
                    .grid_cols(if compact { 1 } else { 2 })
                    .gap(px(12.))
                    .child(self.plot_panel(
                        "速度与钟漂",
                        "共享 epoch 时间轴",
                        velocity_plot(epochs, self.selected_epoch, palette),
                        palette,
                        PlotFocus::Velocity,
                        false,
                        cx,
                    ))
                    .child(self.plot_panel(
                        "高度历史",
                        altitude_statistics(epochs),
                        altitude_plot(epochs, self.selected_epoch, palette),
                        palette,
                        PlotFocus::Altitude,
                        false,
                        cx,
                    )),
            )
            .child(
                div()
                    .mt(px(12.))
                    .rounded(px(6.))
                    .bg(palette.surface)
                    .border_1()
                    .border_color(palette.border)
                    .p(px(14.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(Self::section_title(
                                "当前 PVT",
                                if valid_pvt.is_some() {
                                    pvt.map(|p| format!("{:?}", p.solution_kind))
                                        .unwrap_or_else(|| "等待解算".to_string())
                                } else if pvt.is_some() {
                                    "质量门限拒绝".to_string()
                                } else {
                                    "等待解算".to_string()
                                },
                                palette,
                            ))
                            .child(
                                Button::new("candidate-comparison")
                                    .ghost()
                                    .icon(IconName::PanelRightOpen)
                                    .tooltip("比较 Raw / Smoothed / Kalman")
                                    .disabled(pvt.is_none())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.side_drawer = toggled_side_drawer(
                                            this.side_drawer,
                                            SideDrawer::CandidateComparison,
                                        );
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(10.))
                            .flex()
                            .when(compact, |this| this.flex_col())
                            .child(Self::metric("纬度", latitude, "°", palette))
                            .child(Self::metric("经度", longitude, "°", palette))
                            .child(Self::metric("高程", altitude, "m", palette)),
                    )
                    .child(
                        div()
                            .flex()
                            .when(compact, |this| this.flex_col())
                            .child(Self::metric("三维速度", speed, "m/s", palette))
                            .child(Self::metric("钟漂", clock, "ppm", palette))
                            .child(Self::metric("HDOP", hdop, "", palette)),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .pt(px(10.))
                            .border_t_1()
                            .border_color(palette.border)
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(utc),
                    ),
            )
            .into_any_element()
    }

    fn render_candidate_drawer(
        &self,
        palette: Palette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pvt = self.current_pvt();
        let epoch_number = self.selected_epoch.map(|index| index + 1).unwrap_or(0);
        let epoch_count = self.display_epochs().len();
        div()
            .w(if compact { px(350.) } else { px(430.) })
            .flex_shrink_0()
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

    fn render_timeline_bar(&self, palette: Palette) -> impl IntoElement {
        let epochs = self.display_epochs();
        div()
            .flex_shrink_0()
            .h(px(52.))
            .px(px(20.))
            .bg(palette.surface)
            .border_t_1()
            .border_color(palette.border)
            .flex()
            .items_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(palette.muted)
                    .child(IconName::PanelBottom)
                    .child("EPOCH"),
            )
            .child(
                div()
                    .ml(px(14.))
                    .flex_1()
                    .child(Slider::new(&self.timeline).disabled(epochs.len() < 2)),
            )
            .child(
                div()
                    .ml(px(14.))
                    .min_w(px(72.))
                    .text_right()
                    .text_size(px(10.))
                    .text_color(palette.text)
                    .child(format!(
                        "{} / {}",
                        self.selected_epoch.map(|index| index + 1).unwrap_or(0),
                        epochs.len()
                    )),
            )
    }

    fn plot_panel(
        &self,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        plot: impl IntoElement,
        palette: Palette,
        focus: PlotFocus,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .min_h(if expanded { px(580.) } else { px(250.) })
            .rounded(px(6.))
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .p(px(14.))
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
                                this.focused_plot = if expanded { None } else { Some(focus) };
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .mt(px(12.))
                    .h(if expanded { px(510.) } else { px(200.) })
                    .child(plot),
            )
    }
}

impl Render for GnsscopeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette();
        let viewport_width = f32::from(window.viewport_size().width);
        let compact = viewport_width < 1_180.0;
        let stacked_sidebar = viewport_width < 840.0;
        div()
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .font_family("Segoe UI Variable Text")
            .flex()
            .flex_col()
            .child(self.render_title_bar(palette, compact, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(stacked_sidebar, |this| this.flex_col())
                    .child(self.render_sidebar(palette, stacked_sidebar, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .h_full()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_h_0()
                                            .overflow_y_scrollbar()
                                            .child(self.render_progress(palette))
                                            .child(self.render_overview(palette, compact, cx)),
                                    )
                                    .children(
                                        self.timeline_visible
                                            .then(|| self.render_timeline_bar(palette)),
                                    ),
                            )
                            .children(match self.side_drawer {
                                Some(SideDrawer::Settings) => Some(
                                    self.render_config_drawer(palette, compact, cx)
                                        .into_any_element(),
                                ),
                                Some(SideDrawer::CandidateComparison) => Some(
                                    self.render_candidate_drawer(palette, compact, cx)
                                        .into_any_element(),
                                ),
                                None => None,
                            }),
                    ),
            )
    }
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

fn cn0_plot(
    channels: &[GuiChannelStatus],
    hovered_prn: Option<u8>,
    pinned_prn: Option<u8>,
    palette: Palette,
    cx: &mut Context<GnsscopeApp>,
) -> impl IntoElement {
    let healthy = channels
        .iter()
        .filter(|channel| channel.tracking_locked)
        .map(|channel| (channel.prn, channel.cn0_db_hz))
        .collect::<Vec<_>>();
    div()
        .size_full()
        .flex()
        .items_end()
        .gap(px(3.))
        .px(px(4.))
        .children((1_u8..=32).map(move |prn| {
            let value = healthy
                .iter()
                .find(|(candidate, _)| *candidate == prn)
                .map(|(_, value)| *value)
                .unwrap_or(0.0);
            let height = ((value - 20.0).max(0.0) / 35.0).clamp(0.0, 1.0) as f32;
            let active = hovered_prn.or(pinned_prn) == Some(prn);
            div()
                .id(SharedString::from(format!("cn0-prn-{prn}")))
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .justify_end()
                .items_center()
                .when(value > 0.0, |this| {
                    this.cursor_pointer()
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
                })
                .child(
                    div()
                        .w_full()
                        .max_w(px(16.))
                        .h(gpui::relative(height))
                        .rounded_t(px(2.))
                        .bg(if active {
                            palette.accent
                        } else if value >= 35.0 {
                            palette.healthy
                        } else {
                            palette.warning
                        }),
                )
                .child(
                    div()
                        .h(px(20.))
                        .pt(px(4.))
                        .text_size(px(8.))
                        .text_color(if active {
                            palette.accent
                        } else if value > 0.0 {
                            palette.text
                        } else {
                            palette.muted
                        })
                        .child(format!("{prn}")),
                )
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
}

#[cfg(test)]
mod tests {
    use super::{epoch_index_for_percent, toggled_side_drawer, SettingsCategory, SideDrawer};

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
    fn settings_category_environment_values_are_stable() {
        assert_eq!(SettingsCategory::parse(None), SettingsCategory::Input);
        assert_eq!(
            SettingsCategory::parse(Some("tracking")),
            SettingsCategory::Tracking
        );
        assert_eq!(SettingsCategory::ALL.len(), 5);
        assert_eq!(
            SettingsCategory::ALL.map(SettingsCategory::label),
            ["输入", "捕获", "跟踪", "PVT", "性能"]
        );
    }
}
