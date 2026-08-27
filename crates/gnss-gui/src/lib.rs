//! GUI-facing state and view models for GNSScope.

pub mod app;
pub mod events;
pub mod layout;
pub mod nav_status;
pub mod renderer;
pub mod run_library;
pub mod sample;
pub mod screen;
pub mod settings;
pub mod state;
pub mod view_model;
pub mod worker;

pub use app::GnsscopeApp;
pub use events::GuiReceiverEvent;
pub use layout::{GuiPanel, GuiPanelKind, ReceiverWorkspaceLayout};
pub use nav_status::{GuiNavSourceKind, GuiNavSourceStatus};
pub use renderer::render_workspace_snapshot;
pub use run_library::{ProcessingRun, RunLibrary, RunPersistenceWriter, RunStatus, StoredEpoch};
pub use sample::sample_pvt_view_model;
pub use screen::{
    build_receiver_workspace_screen, GuiMetric, GuiScreenSection, GuiSettingControl,
    GuiSettingControlKind, GuiTable,
};
pub use settings::{GuiIqFormat, GuiProcessingSettings};
pub use state::{GuiChannelStatus, GuiReceiverStatus, GuiReceiverWorkspaceState, GuiSettingsIssue};
pub use view_model::{
    GuiPvtCandidate, GuiPvtCandidateStatus, GuiPvtQuality, GuiPvtTime, GuiPvtVelocity,
    GuiPvtViewModel, GuiSatelliteGeometry,
};
pub use worker::{
    spawn_streaming_worker, ActiveWorker, GuiDiagnosticEvent, GuiDiagnosticEventKind, WorkerEvent,
    WORKER_EVENT_CAPACITY,
};
