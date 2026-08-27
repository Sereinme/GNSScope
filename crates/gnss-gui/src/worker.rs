use crate::{GuiChannelStatus, GuiProcessingSettings, GuiPvtViewModel};
use gnss_core::io::IqReader;
use gnss_core::receiver::{ReceiverChannelState, StreamingPvtEvent, StreamingPvtRunner};
use num_complex::Complex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
    Arc,
};
use std::thread::{self, JoinHandle};

pub const WORKER_EVENT_CAPACITY: usize = 256;

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
        pvt: Box<GuiPvtViewModel>,
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
    sender: SyncSender<WorkerEvent>,
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
    sender: SyncSender<WorkerEvent>,
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
                StreamingPvtEvent::TrackingBatch(updates) => {
                    for update in updates {
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
                        pvt: Box::new(GuiPvtViewModel::from_epoch_output(&output)),
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
    use crate::{GuiIqFormat, RunLibrary, RunStatus};
    use gnss_core::pvt::PvtEpochSolutionKind;
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
        let (sender, receiver) = mpsc::sync_channel(WORKER_EVENT_CAPACITY);
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

    #[test]
    #[ignore = "processes real LabSat IQ to first PVT, or the full file with GNSSCOPE_LABSAT_FULL_FILE"]
    fn real_labsat_gui_settings_acquire_full_constellation_and_produce_pvt() {
        let iq_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/202306202155/Data202306202155(2).dat");
        if !iq_path.exists() {
            eprintln!("SKIP: LabSat Int8 real IQ data not present");
            return;
        }
        let mut settings = GuiProcessingSettings::default();
        assert!(settings.select_iq_file(iq_path));
        if let Some(path) = std::env::var_os("GNSSCOPE_LABSAT_RINEX_NAV") {
            settings.rinex_nav_path = Some(path.into());
        }
        if std::env::var_os("GNSSCOPE_PARALLEL_TRACKING").as_deref() == Some("0".as_ref()) {
            settings.parallel_tracking = false;
        }
        let settings = settings.normalized_for_processing();
        assert!(settings.fixed_prns.is_none());
        let process_full_file = std::env::var_os("GNSSCOPE_LABSAT_FULL_FILE").is_some();

        let (sender, receiver) = mpsc::sync_channel(WORKER_EVENT_CAPACITY);
        let worker = spawn_streaming_worker(settings, sender);
        let mut acquired_prns = BTreeSet::new();
        let mut converged_pvts = Vec::new();
        let mut completed = false;
        while let Ok(event) = receiver.recv() {
            match event {
                WorkerEvent::Diagnostic(event)
                    if event.kind == GuiDiagnosticEventKind::Acquisition =>
                {
                    acquired_prns.extend(event.prn);
                }
                WorkerEvent::Epoch { pvt, .. } if pvt.quality.converged => {
                    converged_pvts.push(pvt);
                    if !process_full_file {
                        worker.cancel();
                    }
                }
                WorkerEvent::Completed { .. } => {
                    completed = true;
                    break;
                }
                WorkerEvent::Cancelled { .. } => break,
                WorkerEvent::Failed(message) => panic!("GUI worker failed: {message}"),
                _ => {}
            }
        }

        let pvt = converged_pvts.last().expect("converged GUI PVT output");
        eprintln!(
            "GUI LabSat final PVT: acquired={acquired_prns:?}, lat={:.8}, lon={:.8}, alt={:.3} m, sats={:?}",
            pvt.latitude_deg,
            pvt.longitude_deg,
            pvt.altitude_m,
            pvt.quality.used_prns
        );
        if process_full_file {
            assert!(completed, "full-file LabSat worker did not complete");
            let altitudes = converged_pvts
                .iter()
                .map(|pvt| pvt.altitude_m)
                .collect::<Vec<_>>();
            let altitude_mean = altitudes.iter().sum::<f64>() / altitudes.len() as f64;
            let altitude_sigma = (altitudes
                .iter()
                .map(|altitude| (altitude - altitude_mean).powi(2))
                .sum::<f64>()
                / altitudes.len() as f64)
                .sqrt();
            let tail = &altitudes[altitudes.len().saturating_sub(20)..];
            let tail_mean = tail.iter().sum::<f64>() / tail.len() as f64;
            let tail_sigma = (tail
                .iter()
                .map(|altitude| (altitude - tail_mean).powi(2))
                .sum::<f64>()
                / tail.len() as f64)
                .sqrt();
            let altitude_min = altitudes.iter().copied().fold(f64::INFINITY, f64::min);
            let altitude_max = altitudes.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            eprintln!(
                "GUI LabSat altitude: converged={}, mean={altitude_mean:.3} m, sigma={altitude_sigma:.3} m, range={altitude_min:.3}..{altitude_max:.3} m; last {} mean={tail_mean:.3} m, sigma={tail_sigma:.3} m, reference=95.000 m",
                altitudes.len(),
                tail.len()
            );
            for kind in [
                PvtEpochSolutionKind::Raw,
                PvtEpochSolutionKind::Smoothed,
                PvtEpochSolutionKind::Kalman,
            ] {
                let candidate_altitudes = converged_pvts
                    .iter()
                    .filter_map(|pvt| pvt.candidate(kind))
                    .filter(|candidate| candidate.quality.converged)
                    .map(|candidate| (candidate.altitude_m, candidate.quality.residual_rms_m))
                    .collect::<Vec<_>>();
                let candidate_tail =
                    &candidate_altitudes[candidate_altitudes.len().saturating_sub(20)..];
                let candidate_altitude_mean = candidate_tail
                    .iter()
                    .map(|(altitude, _)| altitude)
                    .sum::<f64>()
                    / candidate_tail.len() as f64;
                let candidate_altitude_sigma = (candidate_tail
                    .iter()
                    .map(|(altitude, _)| (altitude - candidate_altitude_mean).powi(2))
                    .sum::<f64>()
                    / candidate_tail.len() as f64)
                    .sqrt();
                let candidate_residual_mean = candidate_tail
                    .iter()
                    .map(|(_, residual_rms)| residual_rms)
                    .sum::<f64>()
                    / candidate_tail.len() as f64;
                eprintln!(
                    "GUI LabSat candidate {kind:?}: converged={}, tail={}, altitude_mean={candidate_altitude_mean:.3} m, altitude_sigma={candidate_altitude_sigma:.3} m, residual_rms_mean={candidate_residual_mean:.3} m",
                    candidate_altitudes.len(),
                    candidate_tail.len()
                );
            }
            let mut residuals_by_prn = BTreeMap::<u8, (f64, f64, usize)>::new();
            for pvt in &converged_pvts[converged_pvts.len().saturating_sub(20)..] {
                for satellite in pvt
                    .satellites
                    .iter()
                    .filter(|satellite| satellite.used_in_solution)
                {
                    let entry = residuals_by_prn.entry(satellite.prn).or_default();
                    entry.0 += satellite.residual_m;
                    entry.1 += satellite.elevation_deg;
                    entry.2 += 1;
                }
            }
            for (prn, (residual_sum, elevation_sum, count)) in residuals_by_prn {
                eprintln!(
                    "GUI LabSat satellite G{prn:02}: residual_mean={:.3} m, elevation_mean={:.3} deg, epochs={count}",
                    residual_sum / count as f64,
                    elevation_sum / count as f64
                );
            }
        }
        assert!(acquired_prns.len() >= 4, "acquired PRNs: {acquired_prns:?}");
        assert!(pvt.quality.n_sats >= 4);
        assert!((pvt.latitude_deg - 40.00154).abs() < 0.01);
        assert!((pvt.longitude_deg - 116.33028).abs() < 0.01);
    }

    #[test]
    #[ignore = "processes and persists the complete real CTTC capture"]
    fn real_cttc_gui_worker_persists_complete_run_in_background() {
        let iq_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat",
        );
        if !iq_path.exists() {
            eprintln!("SKIP: CTTC Int16 real IQ data not present");
            return;
        }
        let database_path =
            std::env::temp_dir().join(format!("gnsscope-cttc-worker-{}.sqlite3", Uuid::new_v4()));
        let library = RunLibrary::open(&database_path).expect("open temporary run library");
        let settings = GuiProcessingSettings::cttc_spain(iq_path).normalized_for_processing();
        let run = library.create_run(&settings).expect("create CTTC run");
        let writer = library
            .spawn_persistence_writer()
            .expect("spawn persistence writer");
        let (sender, receiver) = mpsc::sync_channel(WORKER_EVENT_CAPACITY);
        let worker = spawn_streaming_worker(settings, sender);
        let mut processed_samples = 0;
        let mut total_samples = 0;
        let mut epoch_count = 0;
        let mut event_count = 0;

        while let Ok(event) = receiver.recv() {
            match event {
                WorkerEvent::Started {
                    total_samples: total,
                } => total_samples = total,
                WorkerEvent::Progress {
                    processed_samples: processed,
                    total_samples: total,
                    phase,
                } => {
                    processed_samples = processed;
                    total_samples = total;
                    writer
                        .update_progress(
                            run.id,
                            RunStatus::Processing,
                            processed,
                            total,
                            epoch_count,
                            phase,
                        )
                        .expect("queue progress");
                }
                WorkerEvent::Epoch { pvt, channels } => {
                    writer
                        .append_epoch(run.id, epoch_count, *pvt, channels)
                        .expect("queue epoch");
                    epoch_count += 1;
                }
                WorkerEvent::Diagnostic(event) => {
                    writer
                        .append_event(run.id, event_count, event)
                        .expect("queue diagnostic event");
                    event_count += 1;
                }
                WorkerEvent::Completed {
                    processed_samples: processed,
                    total_samples: total,
                } => {
                    processed_samples = processed;
                    total_samples = total;
                    break;
                }
                WorkerEvent::Cancelled { .. } => panic!("CTTC worker was unexpectedly cancelled"),
                WorkerEvent::Failed(message) => panic!("CTTC worker failed: {message}"),
                WorkerEvent::Channels(_) => {}
            }
        }
        writer
            .finalize(
                run.id,
                RunStatus::Completed,
                processed_samples,
                total_samples,
                epoch_count,
                "complete",
            )
            .expect("flush and finalize CTTC run");
        drop(worker);

        let stored_run = library.list_runs().expect("load persisted run").remove(0);
        assert_eq!(stored_run.status, RunStatus::Completed);
        assert_eq!(stored_run.pvt_epoch_count, epoch_count);
        assert_eq!(library.epochs(run.id).expect("load CTTC epochs").len(), 810);
        assert_eq!(
            library.events(run.id).expect("load CTTC events").len(),
            event_count
        );
        drop(writer);
        drop(library);
        let _ = std::fs::remove_file(database_path);
    }
}
