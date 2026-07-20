use crate::{
    GuiChannelStatus, GuiProcessingSettings, GuiPvtQuality, GuiPvtTime, GuiPvtVelocity,
    GuiPvtViewModel, GuiSatelliteGeometry,
};
use chrono::{DateTime, Datelike, NaiveDateTime, Timelike, Utc};
use directories::ProjectDirs;
use gnss_core::pvt::{PvtEpochSolutionKind, UtcDateTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Queued,
    Processing,
    Completed,
    Cancelled,
    Interrupted,
    Failed,
}

impl RunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
            "failed" => Ok(Self::Failed),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessingRun {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub source_path: PathBuf,
    pub status: RunStatus,
    pub progress: f64,
    pub processed_samples: u64,
    pub total_samples: u64,
    pub pvt_epoch_count: usize,
    pub message: String,
    pub settings: GuiProcessingSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEpoch {
    pub epoch_index: usize,
    pub solution_kind: String,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub ecef_m: [f64; 3],
    pub utc: Option<String>,
    pub speed_3d_mps: Option<f64>,
    pub clock_drift_ppm: Option<f64>,
    pub satellites_used: usize,
    pub hdop: f64,
    #[serde(default)]
    pub time: StoredTime,
    #[serde(default)]
    pub velocity: Option<StoredVelocity>,
    #[serde(default)]
    pub quality: StoredQuality,
    #[serde(default)]
    pub channels: Vec<StoredChannel>,
    #[serde(default)]
    pub satellites: Vec<StoredSatellite>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredTime {
    pub gps_week: Option<u16>,
    pub receiver_time_sow: Option<f64>,
    pub corrected_gps_time_sow: Option<f64>,
    pub utc_unix_seconds: Option<f64>,
    pub utc: Option<StoredUtc>,
    pub receiver_clock_bias_us: f64,
    pub receiver_clock_drift_ppm: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StoredUtc {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredVelocity {
    pub east_mps: f64,
    pub north_mps: f64,
    pub up_mps: f64,
    pub speed_mps: f64,
    pub horizontal_speed_mps: f64,
    pub vertical_speed_mps: f64,
    pub heading_deg: Option<f64>,
    pub clock_drift_mps: f64,
    pub residual_rms_mps: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredQuality {
    pub converged: bool,
    pub n_sats: usize,
    pub used_prns: Vec<u8>,
    pub excluded_prns: Vec<u8>,
    pub residual_rms_m: f64,
    pub standardized_residual_rms: f64,
    pub gdop: f64,
    pub hdop: f64,
    pub horizontal_sigma_m: f64,
    pub vertical_sigma_m: f64,
    pub velocity_sigma_mps: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredChannel {
    pub prn: u8,
    pub acquired: bool,
    pub tracking_locked: bool,
    pub nav_locked: bool,
    pub cn0_db_hz: f64,
    pub doppler_hz: f64,
    pub residual_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSatellite {
    pub prn: u8,
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub residual_m: f64,
    pub standardized_residual: f64,
    pub pseudorange_sigma_m: f64,
    pub used_in_solution: bool,
    pub raim_excluded: bool,
}

impl StoredEpoch {
    pub fn from_view_model(value: &GuiPvtViewModel, channels: &[GuiChannelStatus]) -> Self {
        Self {
            epoch_index: value.epoch_index,
            solution_kind: format!("{:?}", value.solution_kind),
            latitude_deg: value.latitude_deg,
            longitude_deg: value.longitude_deg,
            altitude_m: value.altitude_m,
            ecef_m: value.ecef_m,
            utc: value.time.utc.as_ref().map(|utc| {
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:06.3} UTC",
                    utc.year, utc.month, utc.day, utc.hour, utc.minute, utc.second
                )
            }),
            speed_3d_mps: value.velocity.as_ref().map(|velocity| velocity.speed_mps),
            clock_drift_ppm: value.time.receiver_clock_drift_ppm,
            satellites_used: value.quality.n_sats,
            hdop: value.quality.hdop,
            time: StoredTime {
                gps_week: value.time.gps_week,
                receiver_time_sow: value.time.receiver_time_sow,
                corrected_gps_time_sow: value.time.corrected_gps_time_sow,
                utc_unix_seconds: value.time.utc_unix_seconds,
                utc: value.time.utc.map(|utc| StoredUtc {
                    year: utc.year,
                    month: utc.month,
                    day: utc.day,
                    hour: utc.hour,
                    minute: utc.minute,
                    second: utc.second,
                }),
                receiver_clock_bias_us: value.time.receiver_clock_bias_us,
                receiver_clock_drift_ppm: value.time.receiver_clock_drift_ppm,
            },
            velocity: value.velocity.as_ref().map(|velocity| StoredVelocity {
                east_mps: velocity.east_mps,
                north_mps: velocity.north_mps,
                up_mps: velocity.up_mps,
                speed_mps: velocity.speed_mps,
                horizontal_speed_mps: velocity.horizontal_speed_mps,
                vertical_speed_mps: velocity.vertical_speed_mps,
                heading_deg: velocity.heading_deg,
                clock_drift_mps: velocity.clock_drift_mps,
                residual_rms_mps: velocity.residual_rms_mps,
            }),
            quality: StoredQuality {
                converged: value.quality.converged,
                n_sats: value.quality.n_sats,
                used_prns: value.quality.used_prns.clone(),
                excluded_prns: value.quality.excluded_prns.clone(),
                residual_rms_m: value.quality.residual_rms_m,
                standardized_residual_rms: value.quality.standardized_residual_rms,
                gdop: value.quality.gdop,
                hdop: value.quality.hdop,
                horizontal_sigma_m: value.quality.horizontal_sigma_m,
                vertical_sigma_m: value.quality.vertical_sigma_m,
                velocity_sigma_mps: value.quality.velocity_sigma_mps,
            },
            channels: channels.iter().map(StoredChannel::from).collect(),
            satellites: value.satellites.iter().map(StoredSatellite::from).collect(),
        }
    }

    pub fn to_view_model(&self) -> GuiPvtViewModel {
        let solution_kind = match self.solution_kind.as_str() {
            "Raw" => PvtEpochSolutionKind::Raw,
            "Smoothed" => PvtEpochSolutionKind::Smoothed,
            "Kalman" => PvtEpochSolutionKind::Kalman,
            _ => PvtEpochSolutionKind::Unconverged,
        };
        let velocity = self
            .velocity
            .as_ref()
            .map(StoredVelocity::to_view_model)
            .or_else(|| {
                self.speed_3d_mps.map(|speed_mps| GuiPvtVelocity {
                    east_mps: 0.0,
                    north_mps: 0.0,
                    up_mps: 0.0,
                    speed_mps,
                    horizontal_speed_mps: speed_mps,
                    vertical_speed_mps: 0.0,
                    heading_deg: None,
                    clock_drift_mps: 0.0,
                    residual_rms_mps: 0.0,
                })
            });
        GuiPvtViewModel {
            epoch_index: self.epoch_index,
            solution_kind,
            latitude_deg: self.latitude_deg,
            longitude_deg: self.longitude_deg,
            altitude_m: self.altitude_m,
            ecef_m: self.ecef_m,
            time: GuiPvtTime {
                gps_week: self.time.gps_week,
                receiver_time_sow: self.time.receiver_time_sow,
                corrected_gps_time_sow: self.time.corrected_gps_time_sow,
                utc_unix_seconds: self.time.utc_unix_seconds,
                utc: self
                    .time
                    .utc
                    .map(StoredUtc::to_utc)
                    .or_else(|| self.legacy_utc()),
                receiver_clock_bias_us: self.time.receiver_clock_bias_us,
                receiver_clock_drift_ppm: self
                    .time
                    .receiver_clock_drift_ppm
                    .or(self.clock_drift_ppm),
            },
            velocity,
            quality: if self.quality.n_sats > 0 || self.quality.converged {
                self.quality.to_view_model()
            } else {
                GuiPvtQuality {
                    converged: self.solution_kind != "Unconverged",
                    n_sats: self.satellites_used,
                    used_prns: Vec::new(),
                    excluded_prns: Vec::new(),
                    residual_rms_m: 0.0,
                    standardized_residual_rms: 0.0,
                    gdop: 0.0,
                    hdop: self.hdop,
                    horizontal_sigma_m: 0.0,
                    vertical_sigma_m: 0.0,
                    velocity_sigma_mps: None,
                }
            },
            satellites: self
                .satellites
                .iter()
                .map(GuiSatelliteGeometry::from)
                .collect(),
        }
    }

    pub fn channel_view_models(&self) -> Vec<GuiChannelStatus> {
        self.channels.iter().map(GuiChannelStatus::from).collect()
    }

    fn legacy_utc(&self) -> Option<UtcDateTime> {
        let value = self.utc.as_deref()?.strip_suffix(" UTC")?;
        let utc = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f").ok()?;
        Some(UtcDateTime {
            year: utc.year(),
            month: utc.month() as u8,
            day: utc.day() as u8,
            hour: utc.hour() as u8,
            minute: utc.minute() as u8,
            second: utc.second() as f64 + utc.nanosecond() as f64 * 1e-9,
        })
    }
}

impl StoredUtc {
    fn to_utc(self) -> UtcDateTime {
        UtcDateTime {
            year: self.year,
            month: self.month,
            day: self.day,
            hour: self.hour,
            minute: self.minute,
            second: self.second,
        }
    }
}

impl StoredVelocity {
    fn to_view_model(&self) -> GuiPvtVelocity {
        GuiPvtVelocity {
            east_mps: self.east_mps,
            north_mps: self.north_mps,
            up_mps: self.up_mps,
            speed_mps: self.speed_mps,
            horizontal_speed_mps: self.horizontal_speed_mps,
            vertical_speed_mps: self.vertical_speed_mps,
            heading_deg: self.heading_deg,
            clock_drift_mps: self.clock_drift_mps,
            residual_rms_mps: self.residual_rms_mps,
        }
    }
}

impl StoredQuality {
    fn to_view_model(&self) -> GuiPvtQuality {
        GuiPvtQuality {
            converged: self.converged,
            n_sats: self.n_sats,
            used_prns: self.used_prns.clone(),
            excluded_prns: self.excluded_prns.clone(),
            residual_rms_m: self.residual_rms_m,
            standardized_residual_rms: self.standardized_residual_rms,
            gdop: self.gdop,
            hdop: self.hdop,
            horizontal_sigma_m: self.horizontal_sigma_m,
            vertical_sigma_m: self.vertical_sigma_m,
            velocity_sigma_mps: self.velocity_sigma_mps,
        }
    }
}

impl From<&GuiChannelStatus> for StoredChannel {
    fn from(value: &GuiChannelStatus) -> Self {
        Self {
            prn: value.prn,
            acquired: value.acquired,
            tracking_locked: value.tracking_locked,
            nav_locked: value.nav_locked,
            cn0_db_hz: value.cn0_db_hz,
            doppler_hz: value.doppler_hz,
            residual_m: value.residual_m,
        }
    }
}

impl From<&StoredChannel> for GuiChannelStatus {
    fn from(value: &StoredChannel) -> Self {
        Self {
            prn: value.prn,
            acquired: value.acquired,
            tracking_locked: value.tracking_locked,
            nav_locked: value.nav_locked,
            cn0_db_hz: value.cn0_db_hz,
            doppler_hz: value.doppler_hz,
            residual_m: value.residual_m,
        }
    }
}

impl From<&GuiSatelliteGeometry> for StoredSatellite {
    fn from(value: &GuiSatelliteGeometry) -> Self {
        Self {
            prn: value.prn,
            azimuth_deg: value.azimuth_deg,
            elevation_deg: value.elevation_deg,
            residual_m: value.residual_m,
            standardized_residual: value.standardized_residual,
            pseudorange_sigma_m: value.pseudorange_sigma_m,
            used_in_solution: value.used_in_solution,
            raim_excluded: value.raim_excluded,
        }
    }
}

impl From<&StoredSatellite> for GuiSatelliteGeometry {
    fn from(value: &StoredSatellite) -> Self {
        Self {
            prn: value.prn,
            azimuth_deg: value.azimuth_deg,
            elevation_deg: value.elevation_deg,
            residual_m: value.residual_m,
            standardized_residual: value.standardized_residual,
            pseudorange_sigma_m: value.pseudorange_sigma_m,
            used_in_solution: value.used_in_solution,
            raim_excluded: value.raim_excluded,
        }
    }
}

pub struct RunLibrary {
    connection: Connection,
}

impl RunLibrary {
    pub fn open_default() -> rusqlite::Result<Self> {
        let project = ProjectDirs::from("org", "GNSScope", "GNSScope")
            .ok_or(rusqlite::Error::InvalidPath(PathBuf::from("GNSScope")))?;
        std::fs::create_dir_all(project.data_local_dir())
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        Self::open(project.data_local_dir().join("runs.sqlite3"))
    }

    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                source_path TEXT NOT NULL,
                status TEXT NOT NULL,
                progress REAL NOT NULL DEFAULT 0,
                processed_samples INTEGER NOT NULL DEFAULT 0,
                total_samples INTEGER NOT NULL DEFAULT 0,
                pvt_epoch_count INTEGER NOT NULL DEFAULT 0,
                message TEXT NOT NULL DEFAULT '',
                settings_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS epochs (
                run_id TEXT NOT NULL,
                epoch_index INTEGER NOT NULL,
                snapshot_json TEXT NOT NULL,
                PRIMARY KEY (run_id, epoch_index),
                FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );",
        )?;
        connection.execute(
            "UPDATE runs SET status = 'interrupted', message = '应用异常退出，结果不完整' WHERE status = 'processing'",
            [],
        )?;
        connection.execute(
            "UPDATE runs
             SET pvt_epoch_count = (
                SELECT COUNT(*) FROM epochs WHERE epochs.run_id = runs.id
             )
             WHERE pvt_epoch_count != (
                SELECT COUNT(*) FROM epochs WHERE epochs.run_id = runs.id
             )",
            [],
        )?;
        Ok(Self { connection })
    }

    pub fn create_run(&self, settings: &GuiProcessingSettings) -> rusqlite::Result<ProcessingRun> {
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        let source_path = settings.iq_file_path.clone().unwrap_or_default();
        let settings_json = serde_json::to_string(settings)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.connection.execute(
            "INSERT INTO runs (id, created_at, source_path, status, settings_json) VALUES (?1, ?2, ?3, 'queued', ?4)",
            params![id.to_string(), created_at.to_rfc3339(), source_path.to_string_lossy(), settings_json],
        )?;
        Ok(ProcessingRun {
            id,
            created_at,
            source_path,
            status: RunStatus::Queued,
            progress: 0.0,
            processed_samples: 0,
            total_samples: 0,
            pvt_epoch_count: 0,
            message: "等待处理".to_string(),
            settings: settings.clone(),
        })
    }

    pub fn update_progress(
        &self,
        id: Uuid,
        status: RunStatus,
        processed_samples: u64,
        total_samples: u64,
        pvt_epoch_count: usize,
        message: &str,
    ) -> rusqlite::Result<()> {
        let progress = if total_samples == 0 {
            0.0
        } else {
            (processed_samples as f64 / total_samples as f64).clamp(0.0, 1.0)
        };
        self.connection.execute(
            "UPDATE runs SET status = ?2, progress = ?3, processed_samples = ?4, total_samples = ?5, pvt_epoch_count = ?6, message = ?7 WHERE id = ?1",
            params![id.to_string(), status.as_str(), progress, processed_samples as i64, total_samples as i64, pvt_epoch_count as i64, message],
        )?;
        Ok(())
    }

    pub fn append_epoch(
        &self,
        run_id: Uuid,
        epoch_ordinal: usize,
        epoch: &GuiPvtViewModel,
        channels: &[GuiChannelStatus],
    ) -> rusqlite::Result<()> {
        let snapshot = serde_json::to_string(&StoredEpoch::from_view_model(epoch, channels))
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.connection.execute(
            "INSERT OR REPLACE INTO epochs (run_id, epoch_index, snapshot_json) VALUES (?1, ?2, ?3)",
            params![run_id.to_string(), epoch_ordinal as i64, snapshot],
        )?;
        Ok(())
    }

    pub fn list_runs(&self) -> rusqlite::Result<Vec<ProcessingRun>> {
        let mut statement = self.connection.prepare(
            "SELECT id, created_at, source_path, status, progress, processed_samples, total_samples, pvt_epoch_count, message, settings_json FROM runs ORDER BY created_at DESC",
        )?;
        let runs = statement
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let created_at: String = row.get(1)?;
                let status: String = row.get(3)?;
                let settings_json: String = row.get(9)?;
                Ok(ProcessingRun {
                    id: Uuid::parse_str(&id).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?
                        .with_timezone(&Utc),
                    source_path: PathBuf::from(row.get::<_, String>(2)?),
                    status: RunStatus::parse(&status)?,
                    progress: row.get(4)?,
                    processed_samples: row.get::<_, i64>(5)?.max(0) as u64,
                    total_samples: row.get::<_, i64>(6)?.max(0) as u64,
                    pvt_epoch_count: row.get::<_, i64>(7)?.max(0) as usize,
                    message: row.get(8)?,
                    settings: serde_json::from_str(&settings_json)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                })
            })?
            .collect();
        runs
    }

    pub fn latest_epoch(&self, run_id: Uuid) -> rusqlite::Result<Option<StoredEpoch>> {
        let json: Option<String> = self.connection.query_row(
            "SELECT snapshot_json FROM epochs WHERE run_id = ?1 ORDER BY epoch_index DESC LIMIT 1",
            [run_id.to_string()],
            |row| row.get(0),
        ).optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(|_| rusqlite::Error::InvalidQuery))
            .transpose()
    }

    pub fn epochs(&self, run_id: Uuid) -> rusqlite::Result<Vec<StoredEpoch>> {
        let mut statement = self.connection.prepare(
            "SELECT snapshot_json FROM epochs WHERE run_id = ?1 ORDER BY epoch_index ASC",
        )?;
        let epochs = statement
            .query_map([run_id.to_string()], |row| row.get::<_, String>(0))?
            .map(|json| {
                json.and_then(|json| {
                    serde_json::from_str(&json).map_err(|_| rusqlite::Error::InvalidQuery)
                })
            })
            .collect();
        epochs
    }

    pub fn preference(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM preferences WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn set_preference(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT INTO preferences (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_library_persists_immutable_configuration_snapshot() {
        let library = RunLibrary::open(":memory:").expect("open in-memory run library");
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("capture.dat")),
            sample_rate_hz: 5_000_000.0,
            ..Default::default()
        };
        let run = library.create_run(&settings).expect("create run");
        library
            .update_progress(run.id, RunStatus::Processing, 50, 100, 0, "跟踪")
            .expect("update run");

        let stored = library.list_runs().expect("list runs").remove(0);
        assert_eq!(stored.id, run.id);
        assert_eq!(stored.settings.sample_rate_hz, 5_000_000.0);
        assert_eq!(stored.status, RunStatus::Processing);
        assert_eq!(stored.progress, 0.5);
    }

    #[test]
    fn run_library_marks_abandoned_processing_as_interrupted() {
        let path = std::env::temp_dir().join(format!("gnsscope-runs-{}.sqlite3", Uuid::new_v4()));
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("capture.dat")),
            ..Default::default()
        };
        let library = RunLibrary::open(&path).expect("open run library");
        let run = library.create_run(&settings).expect("create run");
        library
            .update_progress(run.id, RunStatus::Processing, 1, 10, 0, "捕获")
            .unwrap();
        drop(library);

        let reopened = RunLibrary::open(&path).expect("reopen run library");
        assert_eq!(
            reopened.list_runs().unwrap()[0].status,
            RunStatus::Interrupted
        );
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn run_library_round_trips_epoch_and_channel_snapshot() {
        let library = RunLibrary::open(":memory:").expect("open in-memory run library");
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("capture.dat")),
            ..Default::default()
        };
        let run = library.create_run(&settings).expect("create run");
        let mut epoch = crate::sample_pvt_view_model();
        epoch.satellites.push(GuiSatelliteGeometry {
            prn: 20,
            azimuth_deg: 123.5,
            elevation_deg: 47.25,
            residual_m: -0.4,
            standardized_residual: -0.2,
            pseudorange_sigma_m: 2.0,
            used_in_solution: true,
            raim_excluded: false,
        });
        let channels = crate::state::sample_channel_statuses();

        library
            .append_epoch(run.id, 0, &epoch, &channels)
            .expect("append epoch");

        let stored = library.epochs(run.id).expect("load epochs");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].channels.len(), channels.len());
        let restored = stored[0].to_view_model();
        assert_eq!(restored.epoch_index, epoch.epoch_index);
        assert_eq!(restored.time.utc, epoch.time.utc);
        assert_eq!(restored.quality.used_prns, epoch.quality.used_prns);
        assert_eq!(restored.satellites, epoch.satellites);
        assert_eq!(
            restored
                .velocity
                .as_ref()
                .map(|velocity| velocity.speed_mps),
            epoch.velocity.as_ref().map(|velocity| velocity.speed_mps)
        );
    }

    #[test]
    fn stored_epoch_reads_legacy_compact_snapshot() {
        let json = r#"{
            "epoch_index": 7,
            "solution_kind": "Raw",
            "latitude_deg": 41.0,
            "longitude_deg": 2.0,
            "altitude_m": 50.0,
            "ecef_m": [1.0, 2.0, 3.0],
            "utc": "2013-04-04 06:24:07.029 UTC",
            "speed_3d_mps": 0.04,
            "clock_drift_ppm": -5.2,
            "satellites_used": 5,
            "hdop": 1.2
        }"#;

        let stored: StoredEpoch = serde_json::from_str(json).expect("read legacy epoch");
        let restored = stored.to_view_model();

        assert_eq!(restored.time.utc.expect("legacy UTC").year, 2013);
        assert_eq!(restored.quality.n_sats, 5);
        assert_eq!(restored.velocity.expect("legacy velocity").speed_mps, 0.04);
        assert!(stored.channels.is_empty());
        assert!(stored.satellites.is_empty());
    }

    #[test]
    fn run_library_does_not_overwrite_repeated_core_epoch_indexes() {
        let library = RunLibrary::open(":memory:").expect("open in-memory run library");
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("capture.dat")),
            ..Default::default()
        };
        let run = library.create_run(&settings).expect("create run");
        let first = crate::sample_pvt_view_model();
        let mut second = first.clone();
        second.latitude_deg += 0.001;

        library.append_epoch(run.id, 0, &first, &[]).unwrap();
        library.append_epoch(run.id, 1, &second, &[]).unwrap();

        let stored = library.epochs(run.id).expect("load epochs");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].latitude_deg, first.latitude_deg);
        assert_eq!(stored[1].latitude_deg, second.latitude_deg);
    }

    #[test]
    fn run_library_reconciles_reported_epoch_count_on_reopen() {
        let path = std::env::temp_dir().join(format!("gnsscope-runs-{}.sqlite3", Uuid::new_v4()));
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("capture.dat")),
            ..Default::default()
        };
        let library = RunLibrary::open(&path).expect("open run library");
        let run = library.create_run(&settings).expect("create run");
        library
            .append_epoch(run.id, 0, &crate::sample_pvt_view_model(), &[])
            .unwrap();
        library
            .update_progress(run.id, RunStatus::Completed, 100, 100, 810, "complete")
            .unwrap();
        drop(library);

        let reopened = RunLibrary::open(&path).expect("reopen run library");
        assert_eq!(reopened.list_runs().unwrap()[0].pvt_epoch_count, 1);
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }
}
