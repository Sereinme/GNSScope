use crate::GuiNavSourceStatus;
use gnss_core::nav::{load_optional_rinex_nav_capabilities, RinexNavConfig};
use gnss_core::pvt::{ContinuousPvtConfig, HatchConfig, IntegerMsAmbiguityConfig, KalmanConfig};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiIqFormat {
    Int8Interleaved,
    Int16Interleaved,
}

#[derive(Debug, Clone)]
pub struct GuiProcessingSettings {
    pub iq_file_path: Option<PathBuf>,
    pub sample_rate_hz: f64,
    pub intermediate_frequency_hz: f64,
    pub iq_format: GuiIqFormat,
    pub rinex_nav_path: Option<PathBuf>,
    pub use_rinex_nav_environment: bool,
    pub auto_discover_colocated_nav: bool,
    pub enable_carrier_smoothing: bool,
    pub enable_kalman: bool,
    pub enable_integer_ms_ambiguity_resolution: bool,
}

impl Default for GuiProcessingSettings {
    fn default() -> Self {
        Self {
            iq_file_path: None,
            sample_rate_hz: 4_000_000.0,
            intermediate_frequency_hz: 0.0,
            iq_format: GuiIqFormat::Int16Interleaved,
            rinex_nav_path: None,
            use_rinex_nav_environment: true,
            auto_discover_colocated_nav: true,
            enable_carrier_smoothing: true,
            enable_kalman: true,
            enable_integer_ms_ambiguity_resolution: true,
        }
    }
}

impl GuiProcessingSettings {
    pub fn rinex_nav_config(&self) -> RinexNavConfig {
        RinexNavConfig {
            explicit_path: self.rinex_nav_path.clone(),
            use_environment: self.use_rinex_nav_environment,
            auto_discover_colocated: self.auto_discover_colocated_nav,
            ..Default::default()
        }
    }

    pub fn nav_source_status_for_iq_path(&self, iq_path: &Path) -> GuiNavSourceStatus {
        load_optional_rinex_nav_capabilities(iq_path, &self.rinex_nav_config())
            .map(GuiNavSourceStatus::from_rinex_nav_capabilities)
            .unwrap_or_else(GuiNavSourceStatus::iq_only)
    }

    pub fn continuous_pvt_config(&self) -> ContinuousPvtConfig {
        ContinuousPvtConfig {
            hatch: self.enable_carrier_smoothing.then(HatchConfig::default),
            integer_ms_ambiguity: self
                .enable_integer_ms_ambiguity_resolution
                .then(IntegerMsAmbiguityConfig::default),
            kalman: self.enable_kalman.then(KalmanConfig::default),
            ..Default::default()
        }
    }

    pub fn is_ready_to_process(&self) -> bool {
        self.iq_file_path.is_some()
            && self.sample_rate_hz.is_finite()
            && self.sample_rate_hz > 0.0
            && self.intermediate_frequency_hz.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_settings_build_rinex_nav_config_for_gui_path() {
        let settings = GuiProcessingSettings {
            rinex_nav_path: Some(PathBuf::from("data/brdc0940.13n")),
            use_rinex_nav_environment: false,
            auto_discover_colocated_nav: false,
            ..Default::default()
        };

        let config = settings.rinex_nav_config();
        let candidates = config.candidates_for_iq_path(Path::new("signal.dat"));

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path, PathBuf::from("data/brdc0940.13n"));
    }

    #[test]
    fn test_settings_keep_iq_only_mode_ready_without_nav_file() {
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            rinex_nav_path: None,
            ..Default::default()
        };

        assert!(settings.is_ready_to_process());
    }

    #[test]
    fn test_settings_disable_optional_continuous_filters() {
        let settings = GuiProcessingSettings {
            enable_carrier_smoothing: false,
            enable_kalman: false,
            enable_integer_ms_ambiguity_resolution: false,
            ..Default::default()
        };

        let config = settings.continuous_pvt_config();

        assert!(config.hatch.is_none());
        assert!(config.kalman.is_none());
        assert!(config.integer_ms_ambiguity.is_none());
    }

    #[test]
    fn test_settings_resolves_nav_source_status_from_rinex_file() {
        let path = std::env::temp_dir().join(format!(
            "gnsscope-gui-rinex-nav-{}-{}.13n",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after Unix epoch")
                .as_nanos()
        ));
        let rinex = "\
     2.10           N: GPS NAV DATA                         RINEX VERSION / TYPE
 0.1212D-07  0.7451D-08 -0.5960D-07 -0.1192D-06          ION ALPHA
 0.9626D+05  0.0000D+00 -0.1966D+06 -0.6554D+05          ION BETA
                                                            END OF HEADER
";
        std::fs::write(&path, rinex).expect("write temporary RINEX nav");
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            rinex_nav_path: Some(path.clone()),
            use_rinex_nav_environment: false,
            auto_discover_colocated_nav: false,
            ..Default::default()
        };

        let status = settings.nav_source_status_for_iq_path(Path::new("signal.dat"));

        let _ = std::fs::remove_file(&path);
        assert_eq!(status.kind, crate::GuiNavSourceKind::ExplicitPath);
        assert_eq!(status.path, Some(path));
        assert!(status.ionosphere_available);
        assert!(!status.ephemeris_available);
    }

    #[test]
    fn test_settings_resolves_iq_only_status_when_nav_missing() {
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            rinex_nav_path: None,
            use_rinex_nav_environment: false,
            auto_discover_colocated_nav: false,
            ..Default::default()
        };

        let status = settings.nav_source_status_for_iq_path(Path::new("signal.dat"));

        assert_eq!(status.kind, crate::GuiNavSourceKind::IqOnly);
        assert!(status.path.is_none());
        assert!(!status.ionosphere_available);
        assert!(!status.ephemeris_available);
    }
}
