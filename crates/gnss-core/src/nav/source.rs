//! Optional navigation-file source configuration.

use crate::nav::ephemeris::{
    parse_rinex_ephemeris_catalog, parse_rinex_ionosphere_params, EphemerisCatalog, IonoParams,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_RINEX_NAV_ENV: &str = "GNSSCOPE_RINEX_NAV";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RinexNavSourceKind {
    ExplicitPath,
    Environment,
    Colocated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RinexNavCandidate {
    pub path: PathBuf,
    pub source_kind: RinexNavSourceKind,
    pub source_label: String,
}

#[derive(Debug, Clone)]
pub struct RinexNavConfig {
    pub explicit_path: Option<PathBuf>,
    pub use_environment: bool,
    pub environment_variable: String,
    pub auto_discover_colocated: bool,
    pub colocated_filenames: Vec<String>,
}

impl Default for RinexNavConfig {
    fn default() -> Self {
        Self {
            explicit_path: None,
            use_environment: true,
            environment_variable: DEFAULT_RINEX_NAV_ENV.to_string(),
            auto_discover_colocated: true,
            colocated_filenames: vec![
                "brdc0940.13n".to_string(),
                "BRDC00IGS_R_20130940000_01D_MN.rnx".to_string(),
            ],
        }
    }
}

impl RinexNavConfig {
    pub fn with_explicit_path(path: impl Into<PathBuf>) -> Self {
        Self {
            explicit_path: Some(path.into()),
            ..Default::default()
        }
    }

    pub fn without_environment(mut self) -> Self {
        self.use_environment = false;
        self
    }

    pub fn candidates_for_iq_path(&self, iq_path: &Path) -> Vec<RinexNavCandidate> {
        let mut candidates = Vec::new();
        if let Some(path) = self.explicit_path.clone() {
            candidates.push(RinexNavCandidate {
                path,
                source_kind: RinexNavSourceKind::ExplicitPath,
                source_label: "rinex-nav-explicit".to_string(),
            });
        }
        if self.use_environment {
            if let Ok(path) = env::var(&self.environment_variable) {
                candidates.push(RinexNavCandidate {
                    path: PathBuf::from(path),
                    source_kind: RinexNavSourceKind::Environment,
                    source_label: "rinex-nav-env".to_string(),
                });
            }
        }
        if self.auto_discover_colocated {
            if let Some(parent) = iq_path.parent() {
                for filename in &self.colocated_filenames {
                    candidates.push(RinexNavCandidate {
                        path: parent.join(filename),
                        source_kind: RinexNavSourceKind::Colocated,
                        source_label: "rinex-nav-colocated".to_string(),
                    });
                }
            }
        }
        candidates
    }
}

#[derive(Debug, Clone)]
pub struct RinexIonoFallback {
    pub iono: IonoParams,
    pub candidate: RinexNavCandidate,
}

#[derive(Debug, Clone)]
pub struct RinexEphemerisFallback {
    pub catalog: EphemerisCatalog,
    pub candidate: RinexNavCandidate,
}

#[derive(Debug, Clone)]
pub struct RinexNavCapabilities {
    pub iono: Option<IonoParams>,
    pub ephemeris_catalog: EphemerisCatalog,
    pub candidate: RinexNavCandidate,
}

pub fn load_optional_rinex_iono(
    iq_path: &Path,
    config: &RinexNavConfig,
) -> Option<RinexIonoFallback> {
    config
        .candidates_for_iq_path(iq_path)
        .into_iter()
        .find_map(|candidate| {
            let contents = read_optional_rinex_nav_text(&candidate.path)?;
            let iono = parse_rinex_ionosphere_params(&contents)?;
            Some(RinexIonoFallback { iono, candidate })
        })
}

pub fn load_optional_rinex_ephemeris_catalog(
    iq_path: &Path,
    config: &RinexNavConfig,
) -> Option<RinexEphemerisFallback> {
    load_optional_rinex_nav_capabilities(iq_path, config).map(|fallback| RinexEphemerisFallback {
        catalog: fallback.ephemeris_catalog,
        candidate: fallback.candidate,
    })
}

pub fn load_optional_rinex_nav_capabilities(
    iq_path: &Path,
    config: &RinexNavConfig,
) -> Option<RinexNavCapabilities> {
    config
        .candidates_for_iq_path(iq_path)
        .into_iter()
        .find_map(|candidate| {
            let contents = read_optional_rinex_nav_text(&candidate.path)?;
            let iono = parse_rinex_ionosphere_params(&contents);
            let catalog = parse_rinex_ephemeris_catalog(&contents);
            (iono.as_ref().is_some_and(|iono| iono.valid) || !catalog.is_empty()).then_some(
                RinexNavCapabilities {
                    iono,
                    ephemeris_catalog: catalog,
                    candidate,
                },
            )
        })
}

pub fn read_optional_rinex_nav_text(path: &Path) -> Option<String> {
    if !path.exists() || is_compressed_rinex_nav_path(path) {
        return None;
    }
    fs::read_to_string(path).ok()
}

pub fn is_compressed_rinex_nav_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let extension = extension.to_ascii_lowercase();
            extension == "z" || extension == "gz"
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rinex_nav_config_prefers_explicit_then_env_then_colocated() {
        let config = RinexNavConfig {
            explicit_path: Some(PathBuf::from("manual.13n")),
            environment_variable: "GNSSCOPE_TEST_RINEX_NAV_MISSING".to_string(),
            colocated_filenames: vec!["brdc0940.13n".to_string()],
            ..Default::default()
        };

        let candidates = config.candidates_for_iq_path(Path::new("data/example/signal.dat"));

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].source_kind, RinexNavSourceKind::ExplicitPath);
        assert_eq!(candidates[0].path, PathBuf::from("manual.13n"));
        assert_eq!(candidates[1].source_kind, RinexNavSourceKind::Colocated);
        assert!(candidates[1]
            .path
            .ends_with(Path::new("data/example/brdc0940.13n")));
    }

    #[test]
    fn test_rinex_nav_config_can_disable_environment() {
        let config = RinexNavConfig {
            environment_variable: "PATH".to_string(),
            colocated_filenames: Vec::new(),
            ..Default::default()
        }
        .without_environment();

        assert!(config
            .candidates_for_iq_path(Path::new("signal.dat"))
            .is_empty());
    }

    #[test]
    fn test_compressed_rinex_nav_paths_are_optional_not_errors() {
        assert!(is_compressed_rinex_nav_path(Path::new("brdc0940.13n.Z")));
        assert!(is_compressed_rinex_nav_path(Path::new(
            "BRDC00IGS_R_20130940000_01D_MN.rnx.gz"
        )));
        assert!(read_optional_rinex_nav_text(Path::new("brdc0940.13n.Z")).is_none());
    }

    #[test]
    fn test_load_optional_rinex_iono_reads_explicit_uncompressed_file() {
        let path = env::temp_dir().join(format!(
            "gnsscope-rinex-nav-{}-{}.13n",
            std::process::id(),
            fastrand::u64(..)
        ));
        let rinex = "\
     2.10           N: GPS NAV DATA                         RINEX VERSION / TYPE
 0.1212D-07  0.7451D-08 -0.5960D-07 -0.1192D-06          ION ALPHA
 0.9626D+05  0.0000D+00 -0.1966D+06 -0.6554D+05          ION BETA
                                                            END OF HEADER
";
        fs::write(&path, rinex).expect("write temporary RINEX nav");

        let config = RinexNavConfig::with_explicit_path(path.clone()).without_environment();
        let loaded = load_optional_rinex_iono(Path::new("signal.dat"), &config)
            .expect("uncompressed RINEX iono should load");

        let _ = fs::remove_file(&path);
        assert!(loaded.iono.valid);
        assert_eq!(
            loaded.candidate.source_kind,
            RinexNavSourceKind::ExplicitPath
        );
        assert!((loaded.iono.alpha[0] - 0.1212e-7).abs() < 1e-18);
    }

    #[test]
    fn test_load_optional_rinex_ephemeris_catalog_reads_explicit_file() {
        let path = env::temp_dir().join(format!(
            "gnsscope-rinex-nav-eph-{}-{}.13n",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after Unix epoch")
                .as_nanos()
        ));
        let rinex = rinex2_single_ephemeris_fixture();
        fs::write(&path, rinex).expect("write temporary RINEX nav");

        let config = RinexNavConfig::with_explicit_path(path.clone()).without_environment();
        let loaded = load_optional_rinex_ephemeris_catalog(Path::new("signal.dat"), &config)
            .expect("uncompressed RINEX ephemeris should load");

        let _ = fs::remove_file(&path);
        assert_eq!(
            loaded.candidate.source_kind,
            RinexNavSourceKind::ExplicitPath
        );
        assert_eq!(loaded.catalog[&1][0].week_number, 1734);
    }

    #[test]
    fn test_load_optional_rinex_nav_capabilities_accepts_iono_only_file() {
        let path = env::temp_dir().join(format!(
            "gnsscope-rinex-nav-cap-{}-{}.13n",
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
        fs::write(&path, rinex).expect("write temporary RINEX nav");

        let config = RinexNavConfig::with_explicit_path(path.clone()).without_environment();
        let loaded = load_optional_rinex_nav_capabilities(Path::new("signal.dat"), &config)
            .expect("RINEX iono-only capabilities should load");

        let _ = fs::remove_file(&path);
        assert!(loaded.iono.expect("iono should be present").valid);
        assert!(loaded.ephemeris_catalog.is_empty());
    }

    #[test]
    fn test_repository_cttc_brdc_nav_loads_when_present() {
        let repo_brdc = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/brdc0940.13n");
        if !repo_brdc.exists() {
            return;
        }

        let config = RinexNavConfig {
            explicit_path: Some(repo_brdc),
            use_environment: false,
            auto_discover_colocated: false,
            ..Default::default()
        };
        let loaded = load_optional_rinex_nav_capabilities(Path::new("signal.dat"), &config)
            .expect("repository CTTC broadcast nav should load");

        assert!(!loaded.ephemeris_catalog.is_empty());
        assert!(
            loaded
                .ephemeris_catalog
                .values()
                .map(Vec::len)
                .sum::<usize>()
                > 10
        );
    }

    fn rinex2_single_ephemeris_fixture() -> &'static str {
        "\
     2              NAVIGATION DATA                         RINEX VERSION / TYPE
                                                            END OF HEADER
 1 13  4  4  0  0  0.0 0.130245462060D-04 0.329691829393D-11 0.000000000000D+00
    0.660000000000D+02 0.110312500000D+02 0.445589989183D-08-0.129411133012D+01
    0.603497028351D-06 0.170189398341D-02 0.977143645287D-05 0.515370147133D+04
    0.345600000000D+06-0.931322574616D-08-0.462984461481D+00-0.931322574615D-07
    0.960438103656D+00 0.193500000000D+03 0.315473100675D+00-0.795818863336D-08
    0.174292974288D-09 0.100000000000D+01 0.173400000000D+04 0.000000000000D+00
    0.200000000000D+01 0.000000000000D+00 0.838190317154D-08 0.660000000000D+02
    0.338418000000D+06 0.400000000000D+01 0.000000000000D+00 0.000000000000D+00
"
    }
}
