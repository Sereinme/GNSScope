use crate::{
    spawn_streaming_worker, ActiveWorker, GuiChannelStatus, GuiProcessingSettings, GuiPvtViewModel,
    ProcessingRun, RunLibrary, RunStatus, StoredEpoch, WorkerEvent,
};
use gpui::prelude::*;
use gpui::{
    bounds, canvas, div, point, px, quad, rgb, size, Bounds, Context, Entity, Hsla, IntoElement,
    ParentElement, PathBuilder, PathPromptOptions, Render, SharedString, Styled, Timer, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    progress::Progress,
    scroll::ScrollableElement,
    slider::{Slider, SliderEvent, SliderState},
    ActiveTheme,
};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use uuid::Uuid;

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

#[derive(Clone, Copy)]
enum ThemePreference {
    System,
    Light,
    Dark,
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
            background: color(0xf4f6f8),
            sidebar: color(0xe9edf1),
            surface: color(0xffffff),
            surface_alt: color(0xf8fafb),
            border: color(0xd7dde3),
            text: color(0x17212b),
            muted: color(0x64717d),
            accent: color(0x087ea4),
            healthy: color(0x16845b),
            warning: color(0xc27b15),
            danger: color(0xc33b3b),
        }
    }

    fn dark() -> Self {
        Self {
            background: color(0x111315),
            sidebar: color(0x181b1e),
            surface: color(0x1d2124),
            surface_alt: color(0x24292d),
            border: color(0x343b40),
            text: color(0xe8edf0),
            muted: color(0x98a4ac),
            accent: color(0x35b4d6),
            healthy: color(0x46b887),
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
    selected_epoch: Option<usize>,
    timeline: Entity<SliderState>,
    follow_live: bool,
    theme_preference: ThemePreference,
    system_dark: bool,
    settings_open: bool,
    status_message: String,
}

impl GnsscopeApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let library = RunLibrary::open_default().ok();
        let history = library
            .as_ref()
            .and_then(|library| library.list_runs().ok())
            .unwrap_or_default();
        let queue = history
            .iter()
            .filter(|run| run.status == RunStatus::Queued)
            .cloned()
            .collect();
        let stored_theme = library
            .as_ref()
            .and_then(|library| library.preference("theme").ok().flatten());
        let theme_preference = std::env::var("GNSSCOPE_THEME")
            .ok()
            .as_deref()
            .map(|value| ThemePreference::parse(Some(value)))
            .unwrap_or_else(|| ThemePreference::parse(stored_theme.as_deref()));
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
        let initial_run_id = history
            .iter()
            .find(|run| run.status != RunStatus::Queued)
            .map(|run| run.id);
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
            selected_epoch,
            timeline,
            follow_live: true,
            theme_preference,
            system_dark: cx.theme().mode.is_dark(),
            settings_open: false,
            status_message: "请选择 IQ 文件开始解算".to_string(),
        };
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

    fn cycle_theme(&mut self) {
        self.theme_preference = self.theme_preference.next();
        if let Some(library) = &self.library {
            let _ = library.set_preference("theme", self.theme_preference.value());
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

    fn apply_cttc_preset(&mut self) {
        let Some(path) = self.settings.iq_file_path.clone() else {
            self.status_message = "请先选择 CTTC IQ 文件".to_string();
            return;
        };
        self.settings = GuiProcessingSettings::cttc_spain(path);
        self.status_message = "已应用 CTTC Spain 4 Msps / Int16 预设".to_string();
    }

    fn enqueue(&mut self) {
        if !self.settings.is_ready_to_process() {
            self.status_message = "请先选择有效的 IQ 文件".to_string();
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

    fn render_sidebar(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
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
            .w(px(258.))
            .min_w(px(220.))
            .h_full()
            .bg(palette.sidebar)
            .border_r_1()
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
                            .label(self.theme_preference.label())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_theme();
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
                    .children(self.queue.iter().map(|run| {
                        div()
                            .mb(px(5.))
                            .px(px(9.))
                            .py(px(8.))
                            .rounded(px(5.))
                            .bg(palette.surface_alt)
                            .text_size(px(11.))
                            .child(
                                run.source_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            )
                    }))
                    .child(
                        div()
                            .mt(px(18.))
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .mb(px(7.))
                            .child("历史记录"),
                    )
                    .children(
                        self.history
                            .iter()
                            .filter(|run| {
                                run.status != RunStatus::Queued
                                    && Some(run.id)
                                        != self.active_run.as_ref().map(|active| active.id)
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
                            }),
                    ),
            )
    }

    fn render_header(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let is_active = self.active_run.is_some();
        let settings = self.display_settings();
        div()
            .px(px(22.))
            .py(px(14.))
            .bg(palette.surface)
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        Button::new("file")
                            .label("选择文件")
                            .on_click(cx.listener(|this, _, _, cx| this.select_file(cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(palette.text)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.file_label()),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(10.))
                                    .text_color(palette.muted)
                                    .child(format!(
                                        "GPS L1 C/A  ·  {:.3} Msps  ·  IF {:.3} MHz  ·  {:?}",
                                        settings.sample_rate_hz / 1e6,
                                        settings.intermediate_frequency_hz / 1e6,
                                        settings.iq_format
                                    )),
                            ),
                    )
                    .children((is_active && !self.follow_live).then(|| {
                        Button::new("return-live")
                            .ghost()
                            .text_color(palette.accent)
                            .label("返回实时")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.return_to_live(window, cx);
                                cx.notify();
                            }))
                    }))
                    .child(
                        Button::new("settings")
                            .ghost()
                            .text_color(palette.text)
                            .label("详细配置")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings_open = !this.settings_open;
                                cx.notify();
                            })),
                    )
                    .child(if is_active {
                        Button::new("cancel")
                            .danger()
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, _| this.cancel_active()))
                    } else {
                        Button::new("run")
                            .primary()
                            .label(if self.selected_run_id.is_some() {
                                "按此配置重算"
                            } else {
                                "开始解算"
                            })
                            .on_click(cx.listener(|this, _, _, _| this.enqueue_selected_or_draft()))
                    }),
            )
    }

    fn render_progress(&self, palette: Palette) -> impl IntoElement {
        let run = self.active_run.as_ref().or_else(|| {
            self.selected_run_id
                .and_then(|id| self.history.iter().find(|run| run.id == id))
                .or_else(|| self.history.first())
        });
        let progress = run.map(|run| run.progress).unwrap_or_else(|| {
            if self
                .history
                .first()
                .is_some_and(|run| run.status == RunStatus::Completed)
            {
                1.0
            } else {
                0.0
            }
        });
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

    fn render_config_drawer(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.display_settings();
        div()
            .w(px(470.))
            .h_full()
            .absolute()
            .right_0()
            .top_0()
            .bg(palette.surface)
            .border_l_1()
            .border_color(palette.border)
            .shadow_lg()
            .p(px(20.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(palette.text)
                    .child("接收机详细配置"),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(if self.selected_run_id.is_some() {
                        "历史任务配置快照（只读）"
                    } else {
                        "任务入队后配置快照冻结"
                    }),
            )
            .children(self.selected_run_id.is_none().then(|| {
                Button::new("cttc-preset")
                    .mt(px(14.))
                    .label("应用 CTTC Spain 预设")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.apply_cttc_preset();
                        cx.notify();
                    }))
            }))
            .children(
                [
                    (
                        "输入与采样",
                        format!("{:?} · chunk {} ms", settings.iq_format, settings.chunk_ms),
                    ),
                    (
                        "捕获",
                        format!(
                            "{} ms × {} · threshold {:.2}",
                            settings.acquisition_window_ms,
                            settings.acquisition_noncoherent_integrations,
                            settings.acquisition_threshold
                        ),
                    ),
                    (
                        "跟踪",
                        format!(
                            "DLL {:.1} Hz · PLL {:.1} Hz",
                            settings.dll_bandwidth_hz, settings.pll_bandwidth_hz
                        ),
                    ),
                    (
                        "锁定判据",
                        format!(
                            "PLL {:.2} · C/N0 {:.1} dB-Hz",
                            settings.minimum_pll_lock_metric, settings.minimum_cn0_db_hz
                        ),
                    ),
                    (
                        "C/N0",
                        format!(
                            "{} samples · α {:.2}",
                            settings.cn0_samples, settings.cn0_smoother_alpha
                        ),
                    ),
                    (
                        "PVT",
                        format!("{} ms · epoch Doppler average", settings.pvt_output_rate_ms),
                    ),
                    (
                        "性能",
                        format!(
                            "acq {} · tracking {}",
                            settings.parallel_acquisition, settings.parallel_tracking
                        ),
                    ),
                ]
                .into_iter()
                .map(|(label, value)| {
                    div()
                        .mt(px(16.))
                        .pb(px(12.))
                        .border_b_1()
                        .border_color(palette.border)
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(palette.text)
                                .child(label),
                        )
                        .child(
                            div()
                                .mt(px(4.))
                                .text_size(px(11.))
                                .text_color(palette.muted)
                                .child(value),
                        )
                }),
            )
    }

    fn render_overview(&self, palette: Palette) -> impl IntoElement {
        let epochs = self.display_epochs();
        let channels = self.display_channels();
        let pvt = self.current_pvt();
        let latitude = pvt
            .map(|p| format!("{:.7}", p.latitude_deg))
            .unwrap_or_else(|| "--".to_string());
        let longitude = pvt
            .map(|p| format!("{:.7}", p.longitude_deg))
            .unwrap_or_else(|| "--".to_string());
        let altitude = pvt
            .map(|p| format!("{:.2}", p.altitude_m))
            .unwrap_or_else(|| "--".to_string());
        let speed = pvt
            .and_then(|p| p.velocity.as_ref())
            .map(|v| format!("{:.3}", v.speed_mps))
            .unwrap_or_else(|| "--".to_string());
        let clock = pvt
            .and_then(|p| p.time.receiver_clock_drift_ppm)
            .map(|v| format!("{v:.4}"))
            .unwrap_or_else(|| "--".to_string());
        let hdop = pvt
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
                    .grid_cols(2)
                    .gap(px(12.))
                    .child(self.plot_panel(
                        "卫星天空图",
                        "真实方位/仰角",
                        sky_plot(palette),
                        palette,
                    ))
                    .child(self.plot_panel(
                        "ENU 位置",
                        "首个有效 epoch 为原点",
                        position_plot(epochs, self.selected_epoch, palette),
                        palette,
                    )),
            )
            .child(div().mt(px(12.)).child(self.plot_panel(
                "稳定通道 C/N0",
                "仅显示 Healthy PRN",
                cn0_plot(channels, palette),
                palette,
            )))
            .child(
                div()
                    .mt(px(12.))
                    .grid()
                    .grid_cols(2)
                    .gap(px(12.))
                    .child(self.plot_panel(
                        "速度与钟漂",
                        "共享 epoch 时间轴",
                        velocity_plot(epochs, self.selected_epoch, palette),
                        palette,
                    ))
                    .child(
                        div()
                            .rounded(px(6.))
                            .bg(palette.surface)
                            .border_1()
                            .border_color(palette.border)
                            .p(px(14.))
                            .child(Self::section_title(
                                "当前 PVT",
                                pvt.map(|p| format!("{:?}", p.solution_kind))
                                    .unwrap_or_else(|| "等待解算".to_string()),
                                palette,
                            ))
                            .child(
                                div()
                                    .mt(px(10.))
                                    .flex()
                                    .child(Self::metric("纬度", latitude, "°", palette))
                                    .child(Self::metric("经度", longitude, "°", palette))
                                    .child(Self::metric("高程", altitude, "m", palette)),
                            )
                            .child(
                                div()
                                    .flex()
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
                    ),
            )
            .child(
                div()
                    .mt(px(12.))
                    .h(px(34.))
                    .rounded(px(5.))
                    .bg(palette.surface_alt)
                    .border_1()
                    .border_color(palette.border)
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(palette.muted)
                            .child("EPOCH"),
                    )
                    .child(
                        div()
                            .ml(px(10.))
                            .flex_1()
                            .child(Slider::new(&self.timeline).disabled(epochs.len() < 2)),
                    )
                    .child(
                        div()
                            .ml(px(10.))
                            .text_size(px(10.))
                            .text_color(palette.text)
                            .child(format!(
                                "{} / {}",
                                self.selected_epoch.map(|i| i + 1).unwrap_or(0),
                                epochs.len()
                            )),
                    ),
            )
    }

    fn plot_panel(
        &self,
        title: &'static str,
        subtitle: &'static str,
        plot: impl IntoElement,
        palette: Palette,
    ) -> impl IntoElement {
        div()
            .min_h(px(250.))
            .rounded(px(6.))
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .p(px(14.))
            .child(Self::section_title(title, subtitle, palette))
            .child(div().mt(px(12.)).h(px(200.)).child(plot))
    }
}

impl Render for GnsscopeApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette();
        div()
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .font_family("Segoe UI")
            .flex()
            .child(self.render_sidebar(palette, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_header(palette, cx))
                    .child(
                        div()
                            .flex_1()
                            .overflow_y_scrollbar()
                            .child(self.render_progress(palette))
                            .child(self.render_overview(palette)),
                    )
                    .children(
                        self.settings_open
                            .then(|| self.render_config_drawer(palette, cx)),
                    ),
            )
    }
}

fn sky_plot(palette: Palette) -> impl IntoElement {
    div()
        .size_full()
        .relative()
        .child(
            canvas(
                move |_, _, _| {},
                move |plot_bounds, _, window, _| {
                    let diameter = plot_bounds.size.width.min(plot_bounds.size.height) - px(16.);
                    let origin = point(
                        plot_bounds.origin.x + (plot_bounds.size.width - diameter) / 2.,
                        plot_bounds.origin.y + (plot_bounds.size.height - diameter) / 2.,
                    );
                    for ratio in [1.0_f32, 0.66, 0.33] {
                        let d = diameter * ratio;
                        let ring = Bounds {
                            origin: point(
                                origin.x + (diameter - d) / 2.,
                                origin.y + (diameter - d) / 2.,
                            ),
                            size: size(d, d),
                        };
                        window.paint_quad(quad(
                            ring,
                            d / 2.,
                            palette.surface_alt,
                            px(1.),
                            palette.border,
                            Default::default(),
                        ));
                    }
                    let center_x = origin.x + diameter / 2.;
                    let center_y = origin.y + diameter / 2.;
                    window.paint_quad(quad(
                        bounds(point(center_x - px(1.), origin.y), size(px(2.), diameter)),
                        px(0.),
                        palette.border,
                        px(0.),
                        palette.border,
                        Default::default(),
                    ));
                    window.paint_quad(quad(
                        bounds(point(origin.x, center_y - px(1.)), size(diameter, px(2.))),
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
        .child(
            div()
                .absolute()
                .bottom(px(6.))
                .w_full()
                .text_center()
                .text_size(px(10.))
                .text_color(palette.muted)
                .child("等待 PVT 输出卫星方位角与仰角"),
        )
}

fn cn0_plot(channels: &[GuiChannelStatus], palette: Palette) -> impl IntoElement {
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
            div()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .justify_end()
                .items_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(16.))
                        .h(gpui::relative(height))
                        .rounded_t(px(2.))
                        .bg(if value >= 35.0 {
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
                        .text_color(if value > 0.0 {
                            palette.text
                        } else {
                            palette.muted
                        })
                        .child(format!("{prn}")),
                )
        }))
}

fn position_plot(
    epochs: &[GuiPvtViewModel],
    selected_epoch: Option<usize>,
    palette: Palette,
) -> impl IntoElement {
    let lla = epochs
        .iter()
        .enumerate()
        .filter(|(_, pvt)| {
            pvt.quality.converged && pvt.latitude_deg.is_finite() && pvt.longitude_deg.is_finite()
        })
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

fn line_plot(
    points: Vec<(f64, f64)>,
    color: Hsla,
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
            let mut builder = PathBuilder::stroke(px(1.5));
            for (index, (x, y)) in points.iter().enumerate() {
                let px_x =
                    plot_bounds.origin.x + plot_bounds.size.width * ((*x - min_x) / span_x) as f32;
                let normalized_y = (y.clamp(min_y, max_y) - min_y) / span_y;
                let px_y =
                    plot_bounds.origin.y + plot_bounds.size.height * (1.0 - normalized_y as f32);
                if index == 0 {
                    builder.move_to(point(px_x, px_y));
                } else {
                    builder.line_to(point(px_x, px_y));
                }
            }
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
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

fn epoch_index_for_percent(count: usize, percent: f32) -> Option<usize> {
    (count > 0).then(|| ((percent.clamp(0.0, 100.0) / 100.0) * (count - 1) as f32).round() as usize)
}

#[cfg(test)]
mod tests {
    use super::epoch_index_for_percent;

    #[test]
    fn timeline_percent_maps_to_shared_epoch_index() {
        assert_eq!(epoch_index_for_percent(0, 50.0), None);
        assert_eq!(epoch_index_for_percent(1, 0.0), Some(0));
        assert_eq!(epoch_index_for_percent(101, 0.0), Some(0));
        assert_eq!(epoch_index_for_percent(101, 50.0), Some(50));
        assert_eq!(epoch_index_for_percent(101, 100.0), Some(100));
    }
}
