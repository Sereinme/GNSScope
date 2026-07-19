use crate::{GuiChannelStatus, GuiNavSourceStatus, GuiProcessingSettings, GuiPvtViewModel};

#[derive(Debug, Clone)]
pub enum GuiReceiverEvent {
    SettingsChanged(Box<GuiProcessingSettings>),
    ProcessingStarted,
    Progress { fraction: f32, message: String },
    NavSourceUpdated(GuiNavSourceStatus),
    ChannelUpdated(GuiChannelStatus),
    PvtUpdated(Box<GuiPvtViewModel>),
    ProcessingCompleted,
    ProcessingFailed(String),
}

impl GuiReceiverEvent {
    pub fn settings_changed(settings: GuiProcessingSettings) -> Self {
        Self::SettingsChanged(Box::new(settings))
    }

    pub fn pvt_updated(pvt: GuiPvtViewModel) -> Self {
        Self::PvtUpdated(Box::new(pvt))
    }

    pub fn nav_source_updated(nav_source: GuiNavSourceStatus) -> Self {
        Self::NavSourceUpdated(nav_source)
    }
}
