use crate::GuiNavSourceStatus;
use gnss_core::io::IqFormat;
use gnss_core::nav::{load_optional_rinex_nav_capabilities, RinexNavConfig};
use gnss_core::pvt::{ContinuousPvtConfig, HatchConfig, IntegerMsAmbiguityConfig, KalmanConfig};
use gnss_core::receiver::{AcquisitionSchedulerConfig, ChannelAdmissionConfig, StreamingPvtConfig};
use gnss_core::tracking::TrackingConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuiIqFormat {
    Int8Interleaved,
    Int16Interleaved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    pub enable_raim: bool,
    pub initial_position_lla_deg_m: Option<[f64; 3]>,
    pub chunk_ms: usize,
    pub pvt_output_rate_ms: u64,
    pub average_doppler_over_pvt_epoch: bool,
    pub acquisition_window_ms: usize,
    pub acquisition_noncoherent_integrations: usize,
    pub acquisition_threshold: f64,
    pub cold_acquisition_interval_ms: u64,
    pub acquisition_doppler_max_hz: f64,
    pub acquisition_doppler_step_hz: f64,
    pub reacquisition_interval_ms: u64,
    pub max_acquisition_prns_per_scan: usize,
    pub dll_bandwidth_hz: f64,
    pub pll_bandwidth_hz: f64,
    pub early_late_spacing_chips: f64,
    pub carrier_aiding: bool,
    pub cn0_samples: usize,
    pub cn0_smoother_alpha: f64,
    pub minimum_pll_lock_metric: f64,
    pub minimum_cn0_db_hz: f64,
    pub probation_good_epochs: usize,
    pub parallel_acquisition: bool,
    pub parallel_tracking: bool,
    pub fixed_prns: Option<Vec<u8>>,
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
            enable_raim: false,
            initial_position_lla_deg_m: None,
            chunk_ms: 20,
            pvt_output_rate_ms: 1_000,
            average_doppler_over_pvt_epoch: true,
            acquisition_window_ms: 10,
            acquisition_noncoherent_integrations: 10,
            acquisition_threshold: 3.0,
            cold_acquisition_interval_ms: 10_000,
            acquisition_doppler_max_hz: 10_000.0,
            acquisition_doppler_step_hz: 250.0,
            reacquisition_interval_ms: 1_000,
            max_acquisition_prns_per_scan: 4,
            dll_bandwidth_hz: 4.0,
            pll_bandwidth_hz: 50.0,
            early_late_spacing_chips: 0.5,
            carrier_aiding: true,
            cn0_samples: 20,
            cn0_smoother_alpha: 0.1,
            minimum_pll_lock_metric: 0.65,
            minimum_cn0_db_hz: 25.0,
            probation_good_epochs: 3,
            parallel_acquisition: true,
            parallel_tracking: true,
            fixed_prns: None,
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
        let mut config = ContinuousPvtConfig {
            hatch: self.enable_carrier_smoothing.then(HatchConfig::default),
            integer_ms_ambiguity: self
                .enable_integer_ms_ambiguity_resolution
                .then(IntegerMsAmbiguityConfig::default),
            initial_position_lla_rad: self.initial_position_lla_deg_m.map(
                |[latitude_deg, longitude_deg, altitude_m]| {
                    [
                        latitude_deg.to_radians(),
                        longitude_deg.to_radians(),
                        altitude_m,
                    ]
                },
            ),
            kalman: self.enable_kalman.then(KalmanConfig::default),
            ..Default::default()
        };
        if !self.enable_raim {
            config.wls.raim_max_exclusions = 0;
        }
        config
    }

    pub fn normalized_for_processing(&self) -> Self {
        let mut settings = self.clone();
        if settings.is_cttc_spain_reference_capture() {
            settings.enable_integer_ms_ambiguity_resolution = true;
            settings
                .initial_position_lla_deg_m
                .get_or_insert(StreamingPvtConfig::CTTC_SPAIN_APPROX_LLA_DEG_M);
        }
        settings
    }

    fn is_cttc_spain_reference_capture(&self) -> bool {
        let file_matches = self
            .iq_file_path
            .as_ref()
            .and_then(|path| path.file_name())
            .is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case("2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat")
            });
        let signal_matches = (self.sample_rate_hz - 4_000_000.0).abs() < f64::EPSILON
            && self.intermediate_frequency_hz.abs() < f64::EPSILON
            && self.iq_format == GuiIqFormat::Int16Interleaved
            && self.fixed_prns.as_deref()
                == Some(StreamingPvtConfig::CTTC_SPAIN_GPS_L1_CA_PRNS.as_slice());
        file_matches && signal_matches
    }

    pub fn is_ready_to_process(&self) -> bool {
        crate::state::validate_settings(self).is_empty()
    }

    pub fn streaming_pvt_config(&self) -> StreamingPvtConfig {
        let acquisition = AcquisitionSchedulerConfig {
            cold_acq_interval_ms: self.cold_acquisition_interval_ms.max(1),
            acq_window_ms: self.acquisition_window_ms.max(1),
            noncoherent_integrations: self.acquisition_noncoherent_integrations.max(1),
            threshold: self.acquisition_threshold,
            doppler_max_hz: self.acquisition_doppler_max_hz,
            doppler_step_hz: self.acquisition_doppler_step_hz,
            reacq_interval_ms: self.reacquisition_interval_ms.max(1),
            max_acq_prns_per_scan: self.max_acquisition_prns_per_scan.max(1),
            fixed_prns: self.fixed_prns.clone(),
            ..Default::default()
        };
        let tracking = TrackingConfig {
            fs: self.sample_rate_hz,
            if_freq: self.intermediate_frequency_hz,
            dll_bw: self.dll_bandwidth_hz,
            pll_bw: self.pll_bandwidth_hz,
            early_late_spc: self.early_late_spacing_chips,
            carrier_aiding: self.carrier_aiding,
            cn0_samples: self.cn0_samples.max(1),
            cn0_smoother_alpha: self.cn0_smoother_alpha.clamp(0.0, 1.0),
            ..Default::default()
        };
        let channel_admission = ChannelAdmissionConfig {
            probation_good_epochs: self.probation_good_epochs.max(1),
            min_pll_lock_metric: self.minimum_pll_lock_metric,
            min_cn0_db_hz: self.minimum_cn0_db_hz,
            ..Default::default()
        };
        StreamingPvtConfig {
            iq_path: self.iq_file_path.clone(),
            iq_format: match self.iq_format {
                GuiIqFormat::Int8Interleaved => IqFormat::Int8,
                GuiIqFormat::Int16Interleaved => IqFormat::Int16,
            },
            sample_rate_hz: self.sample_rate_hz,
            intermediate_frequency_hz: self.intermediate_frequency_hz,
            chunk_ms: self.chunk_ms.max(1),
            pvt_output_rate_ms: self.pvt_output_rate_ms.max(1),
            average_doppler_over_pvt_epoch: self.average_doppler_over_pvt_epoch,
            fixed_prns: self.fixed_prns.clone(),
            rinex_nav: self.rinex_nav_config(),
            acquisition_ring_ms: acquisition.acq_window_ms,
            acquisition,
            tracking,
            channel_admission,
            parallel_acquisition: self.parallel_acquisition,
            parallel_tracking: self.parallel_tracking,
            continuous_pvt: self.continuous_pvt_config(),
            ..Default::default()
        }
    }

    pub fn cttc_spain(iq_path: impl Into<PathBuf>) -> Self {
        let fixed_prns = StreamingPvtConfig::CTTC_SPAIN_GPS_L1_CA_PRNS.to_vec();
        Self {
            iq_file_path: Some(iq_path.into()),
            sample_rate_hz: 4_000_000.0,
            intermediate_frequency_hz: 0.0,
            iq_format: GuiIqFormat::Int16Interleaved,
            chunk_ms: 20,
            pvt_output_rate_ms: 100,
            acquisition_window_ms: 10,
            acquisition_noncoherent_integrations: 10,
            acquisition_threshold: 3.0,
            cold_acquisition_interval_ms: 1,
            max_acquisition_prns_per_scan: fixed_prns.len(),
            probation_good_epochs: 1,
            minimum_cn0_db_hz: 0.0,
            minimum_pll_lock_metric: 0.0,
            enable_carrier_smoothing: false,
            enable_kalman: false,
            enable_integer_ms_ambiguity_resolution: true,
            enable_raim: false,
            initial_position_lla_deg_m: Some(StreamingPvtConfig::CTTC_SPAIN_APPROX_LLA_DEG_M),
            fixed_prns: Some(fixed_prns),
            ..Default::default()
        }
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
            enable_raim: false,
            ..Default::default()
        };

        let config = settings.continuous_pvt_config();

        assert!(config.hatch.is_none());
        assert!(config.kalman.is_none());
        assert!(config.integer_ms_ambiguity.is_none());
        assert_eq!(config.wls.raim_max_exclusions, 0);
    }

    #[test]
    fn test_settings_enable_raim_with_wls_defaults() {
        let settings = GuiProcessingSettings {
            enable_raim: true,
            ..Default::default()
        };

        let config = settings.continuous_pvt_config();

        assert_eq!(
            config.wls.raim_max_exclusions,
            gnss_core::pvt::WlsConfig::default().raim_max_exclusions
        );
        assert!(config.wls.raim_max_exclusions > 0);
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

    #[test]
    fn test_settings_map_visible_receiver_controls() {
        let settings = GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            iq_format: GuiIqFormat::Int8Interleaved,
            pvt_output_rate_ms: 250,
            acquisition_threshold: 4.25,
            pll_bandwidth_hz: 32.0,
            cn0_samples: 40,
            parallel_tracking: false,
            ..Default::default()
        };

        let config = settings.streaming_pvt_config();

        assert_eq!(config.iq_format, IqFormat::Int8);
        assert_eq!(config.pvt_output_rate_ms, 250);
        assert_eq!(config.acquisition.threshold, 4.25);
        assert_eq!(config.tracking.pll_bw, 32.0);
        assert_eq!(config.tracking.cn0_samples, 40);
        assert!(!config.parallel_tracking);
        assert!(config.average_doppler_over_pvt_epoch);
    }

    #[test]
    fn test_cttc_preset_matches_streaming_reference() {
        let settings = GuiProcessingSettings::cttc_spain("cttc.dat");
        let config = settings.streaming_pvt_config();

        assert_eq!(config.fixed_prns, Some(vec![1, 11, 17, 20, 32]));
        assert_eq!(config.acquisition.fixed_prns, config.fixed_prns);
        assert_eq!(config.chunk_ms, 20);
        assert_eq!(config.pvt_output_rate_ms, 100);
        assert_eq!(config.acquisition.cold_acq_interval_ms, 1);
        assert_eq!(config.channel_admission.probation_good_epochs, 1);
        assert_eq!(config.channel_admission.min_cn0_db_hz, 0.0);
        assert_eq!(config.channel_admission.min_pll_lock_metric, 0.0);
        assert!(config.continuous_pvt.hatch.is_none());
        assert!(config.continuous_pvt.kalman.is_none());
        assert!(config.continuous_pvt.integer_ms_ambiguity.is_some());
        assert!(config.continuous_pvt.initial_position_lla_rad.is_some());
        assert_eq!(config.continuous_pvt.wls.raim_max_exclusions, 0);
    }

    #[test]
    fn test_legacy_cttc_settings_restore_coarse_position_prior() {
        let settings: GuiProcessingSettings = serde_json::from_str(
            r#"{
                "iq_file_path":"data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat",
                "sample_rate_hz":4000000.0,
                "intermediate_frequency_hz":0.0,
                "iq_format":"Int16Interleaved",
                "enable_integer_ms_ambiguity_resolution":false,
                "fixed_prns":[1,11,17,20,32]
            }"#,
        )
        .expect("deserialize legacy CTTC settings");

        assert!(!settings.enable_integer_ms_ambiguity_resolution);
        let normalized = settings.normalized_for_processing();
        let config = normalized.streaming_pvt_config();
        let prior = config
            .continuous_pvt
            .initial_position_lla_rad
            .expect("CTTC compatibility prior");

        assert!((prior[0].to_degrees() - 41.2747).abs() < 1e-9);
        assert!((prior[1].to_degrees() - 1.9875).abs() < 1e-9);
        assert_eq!(prior[2], 60.0);
        assert!(normalized.enable_integer_ms_ambiguity_resolution);
        assert!(config.continuous_pvt.integer_ms_ambiguity.is_some());
    }
}
