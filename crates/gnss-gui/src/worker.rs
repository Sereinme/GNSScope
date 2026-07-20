use crate::{GuiChannelStatus, GuiProcessingSettings, GuiPvtViewModel};
use gnss_core::io::IqReader;
use gnss_core::receiver::{ReceiverChannelState, StreamingPvtEvent, StreamingPvtRunner};
use num_complex::Complex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiDiagnosticEventKind {
    Acquisition,
    AcquisitionFailed,
    Reacquisition,
    ReacquisitionFailed,
    ChannelState,
    Navigation,
}

impl GuiDiagnosticEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Acquisition => "acquisition",
            Self::AcquisitionFailed => "acquisition_failed",
            Self::Reacquisition => "reacquisition",
            Self::ReacquisitionFailed => "reacquisition_failed",
            Self::ChannelState => "channel_state",
            Self::Navigation => "navigation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "acquisition" => Some(Self::Acquisition),
            "acquisition_failed" => Some(Self::AcquisitionFailed),
            "reacquisition" => Some(Self::Reacquisition),
            "reacquisition_failed" => Some(Self::ReacquisitionFailed),
            "channel_state" => Some(Self::ChannelState),
            "navigation" => Some(Self::Navigation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiDiagnosticEvent {
    pub now_ms: u64,
    pub sample_index: u64,
    pub prn: Option<u8>,
    pub kind: GuiDiagnosticEventKind,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    Started {
        total_samples: u64,
    },
    Progress {
        processed_samples: u64,
        total_samples: u64,
        phase: String,
    },
    Epoch {
        pvt: GuiPvtViewModel,
        channels: Vec<GuiChannelStatus>,
    },
    Channels(Vec<GuiChannelStatus>),
    Diagnostic(GuiDiagnosticEvent),
    Cancelled {
        processed_samples: u64,
        total_samples: u64,
    },
    Completed {
        processed_samples: u64,
        total_samples: u64,
    },
    Failed(String),
}

pub struct ActiveWorker {
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ActiveWorker {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Drop for ActiveWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn spawn_streaming_worker(
    settings: GuiProcessingSettings,
    sender: Sender<WorkerEvent>,
) -> ActiveWorker {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let handle = thread::spawn(move || run_streaming_worker(settings, worker_cancel, sender));
    ActiveWorker {
        cancel,
        handle: Some(handle),
    }
}

fn run_streaming_worker(
    settings: GuiProcessingSettings,
    cancel: Arc<AtomicBool>,
    sender: Sender<WorkerEvent>,
) {
    let config = settings.streaming_pvt_config();
    let Some(path) = config.iq_path.clone() else {
        let _ = sender.send(WorkerEvent::Failed("未选择 IQ 文件".to_string()));
        return;
    };
    let mut reader = match IqReader::open(&path, config.iq_format) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = sender.send(WorkerEvent::Failed(format!("无法读取 IQ 文件：{error}")));
            return;
        }
    };
    let total_samples = reader.total_samples();
    let _ = sender.send(WorkerEvent::Started { total_samples });
    let chunk_samples = config.chunk_samples().max(1);
    let mut runner = StreamingPvtRunner::new(config);
    let mut buffer = vec![Complex::new(0.0, 0.0); chunk_samples];
    let mut processed_samples = 0_u64;
    let mut channels = BTreeMap::<u8, GuiChannelStatus>::new();
    let mut channel_states = BTreeMap::<u8, ReceiverChannelState>::new();
    let mut acquired_prns = BTreeSet::new();
    let mut nav_subframes = 0_usize;
    let mut pvt_epochs = 0_usize;
    let channel_epoch_samples = runner.config().doppler_epoch_window_samples().max(1);
    let mut next_channel_epoch = channel_epoch_samples;

    loop {
        if cancel.load(Ordering::Acquire) {
            let _ = sender.send(WorkerEvent::Cancelled {
                processed_samples,
                total_samples,
            });
            return;
        }
        let samples_read = match reader.read(&mut buffer, chunk_samples) {
            Ok(0) => break,
            Ok(samples_read) => samples_read,
            Err(error) => {
                let _ = sender.send(WorkerEvent::Failed(format!("IQ 读取失败：{error}")));
                return;
            }
        };
        let events = runner.process_stream_samples(&buffer[..samples_read], processed_samples);
        processed_samples += samples_read as u64;
        for event in events {
            match event {
                StreamingPvtEvent::AcquisitionScanCompleted(scan) => {
                    let now_ms = (processed_samples as f64 * 1_000.0 / settings.sample_rate_hz)
                        .round() as u64;
                    for result in scan.results {
                        let seen_before = acquired_prns.contains(&result.prn);
                        let kind = match (seen_before, result.detected) {
                            (false, true) => GuiDiagnosticEventKind::Acquisition,
                            (false, false) => GuiDiagnosticEventKind::AcquisitionFailed,
                            (true, true) => GuiDiagnosticEventKind::Reacquisition,
                            (true, false) => GuiDiagnosticEventKind::ReacquisitionFailed,
                        };
                        if result.detected {
                            acquired_prns.insert(result.prn);
                        }
                        let _ = sender.send(WorkerEvent::Diagnostic(GuiDiagnosticEvent {
                            now_ms,
                            sample_index: scan.scan.sample_index,
                            prn: Some(result.prn),
                            kind,
                            detail: if result.detected {
                                format!(
                                    "Doppler {:+.0} Hz · metric {:.2}",
                                    result.doppler_hz, result.peak_to_mean
                                )
                            } else {
                                format!("未过阈值 · metric {:.2}", result.peak_to_mean)
                            },
                        }));
                    }
                }
                StreamingPvtEvent::TrackingUpdated(update) => {
                    let previous_state = channel_states.insert(update.prn, update.state);
                    if previous_state != Some(update.state) {
                        let _ = sender.send(WorkerEvent::Diagnostic(GuiDiagnosticEvent {
                            now_ms: update.now_ms,
                            sample_index: processed_samples,
                            prn: Some(update.prn),
                            kind: GuiDiagnosticEventKind::ChannelState,
                            detail: format!("{:?} -> {:?}", previous_state, update.state),
                        }));
                    }
                    let healthy = update.state == ReceiverChannelState::Healthy;
                    channels.insert(
                        update.prn,
                        GuiChannelStatus {
                            prn: update.prn,
                            acquired: !matches!(
                                update.state,
                                ReceiverChannelState::Absent | ReceiverChannelState::Acquiring
                            ),
                            tracking_locked: healthy,
                            nav_locked: healthy,
                            cn0_db_hz: update.output.cn0_db_hz,
                            doppler_hz: update.output.carrier_doppler_hz,
                            residual_m: None,
                        },
                    );
                }
                StreamingPvtEvent::NavSubframeDecoded(decoded) => {
                    nav_subframes += 1;
                    let _ = sender.send(WorkerEvent::Diagnostic(GuiDiagnosticEvent {
                        now_ms: decoded.now_ms,
                        sample_index: decoded.rx_sample,
                        prn: Some(decoded.prn),
                        kind: GuiDiagnosticEventKind::Navigation,
                        detail: format!(
                            "子帧 {} · TOW count {}",
                            decoded.subframe_id, decoded.tow_count
                        ),
                    }));
                }
                StreamingPvtEvent::PvtOutput(output) => {
                    pvt_epochs += 1;
                    let _ = sender.send(WorkerEvent::Epoch {
                        pvt: GuiPvtViewModel::from_epoch_output(&output),
                        channels: channels.values().cloned().collect(),
                    });
                }
                _ => {}
            }
        }
        let emit_epoch =
            processed_samples >= next_channel_epoch || processed_samples >= total_samples;
        if emit_epoch {
            while processed_samples >= next_channel_epoch {
                next_channel_epoch += channel_epoch_samples;
            }
            let _ = sender.send(WorkerEvent::Channels(channels.values().cloned().collect()));
        }
        if emit_epoch {
            let healthy_channels = channels
                .values()
                .filter(|channel| channel.tracking_locked)
                .count();
            let phase = format!(
                "捕获 {}/{} · Healthy {} · 导航子帧 {} · PVT {}",
                acquired_prns.len(),
                settings.fixed_prns.as_ref().map(Vec::len).unwrap_or(32),
                healthy_channels,
                nav_subframes,
                pvt_epochs
            );
            if sender
                .send(WorkerEvent::Progress {
                    processed_samples,
                    total_samples,
                    phase,
                })
                .is_err()
            {
                return;
            }
        }
    }

    let _ = sender.send(WorkerEvent::Completed {
        processed_samples,
        total_samples,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiIqFormat;
    use std::io::Write;
    use std::sync::mpsc;
    use uuid::Uuid;

    #[test]
    fn worker_reports_monotonic_progress_for_short_iq_file() {
        let path = std::env::temp_dir().join(format!("gnsscope-worker-{}.iq", Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).expect("create IQ file");
        file.write_all(&vec![0_u8; 8_000]).expect("write IQ file");
        drop(file);
        let settings = GuiProcessingSettings {
            iq_file_path: Some(path.clone()),
            sample_rate_hz: 1_000.0,
            iq_format: GuiIqFormat::Int8Interleaved,
            chunk_ms: 2,
            acquisition_window_ms: 10,
            ..Default::default()
        };
        let (sender, receiver) = mpsc::channel();
        let worker = spawn_streaming_worker(settings, sender);
        let events: Vec<_> = receiver.iter().collect();
        drop(worker);
        let progress: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                WorkerEvent::Progress {
                    processed_samples, ..
                } => Some(*processed_samples),
                _ => None,
            })
            .collect();
        assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(matches!(events.last(), Some(WorkerEvent::Completed { .. })));
        let _ = std::fs::remove_file(path);
    }
}
