use crate::{GuiProcessingSettings, GuiPvtViewModel};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
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
}

impl From<&GuiPvtViewModel> for StoredEpoch {
    fn from(value: &GuiPvtViewModel) -> Self {
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

    pub fn append_epoch(&self, run_id: Uuid, epoch: &GuiPvtViewModel) -> rusqlite::Result<()> {
        let snapshot = serde_json::to_string(&StoredEpoch::from(epoch))
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.connection.execute(
            "INSERT OR REPLACE INTO epochs (run_id, epoch_index, snapshot_json) VALUES (?1, ?2, ?3)",
            params![run_id.to_string(), epoch.epoch_index as i64, snapshot],
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
}
