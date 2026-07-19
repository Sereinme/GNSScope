use gnss_core::nav::{
    RinexEphemerisFallback, RinexIonoFallback, RinexNavCandidate, RinexNavCapabilities,
    RinexNavSourceKind,
};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiNavSourceKind {
    IqOnly,
    ExplicitPath,
    Environment,
    Colocated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiNavSourceStatus {
    pub kind: GuiNavSourceKind,
    pub label: String,
    pub path: Option<PathBuf>,
    pub ionosphere_available: bool,
    pub ephemeris_available: bool,
}

impl GuiNavSourceStatus {
    pub fn iq_only() -> Self {
        Self {
            kind: GuiNavSourceKind::IqOnly,
            label: "IQ-only mode".to_string(),
            path: None,
            ionosphere_available: false,
            ephemeris_available: false,
        }
    }

    pub fn from_rinex_candidate(candidate: RinexNavCandidate, ionosphere_available: bool) -> Self {
        Self::from_rinex_capabilities(candidate, ionosphere_available, false)
    }

    pub fn from_rinex_capabilities(
        candidate: RinexNavCandidate,
        ionosphere_available: bool,
        ephemeris_available: bool,
    ) -> Self {
        let kind = match candidate.source_kind {
            RinexNavSourceKind::ExplicitPath => GuiNavSourceKind::ExplicitPath,
            RinexNavSourceKind::Environment => GuiNavSourceKind::Environment,
            RinexNavSourceKind::Colocated => GuiNavSourceKind::Colocated,
        };
        let label = match kind {
            GuiNavSourceKind::IqOnly => "IQ-only mode",
            GuiNavSourceKind::ExplicitPath => "Explicit RINEX nav",
            GuiNavSourceKind::Environment => "Environment RINEX nav",
            GuiNavSourceKind::Colocated => "Colocated RINEX nav",
        }
        .to_string();
        Self {
            kind,
            label,
            path: Some(candidate.path),
            ionosphere_available,
            ephemeris_available,
        }
    }

    pub fn from_rinex_iono_fallback(fallback: RinexIonoFallback) -> Self {
        Self::from_rinex_candidate(fallback.candidate, fallback.iono.valid)
    }

    pub fn from_rinex_ephemeris_fallback(fallback: RinexEphemerisFallback) -> Self {
        Self::from_rinex_capabilities(fallback.candidate, false, !fallback.catalog.is_empty())
    }

    pub fn from_rinex_nav_capabilities(fallback: RinexNavCapabilities) -> Self {
        let ionosphere_available = fallback.iono.as_ref().is_some_and(|iono| iono.valid);
        let ephemeris_available = !fallback.ephemeris_catalog.is_empty();
        Self::from_rinex_capabilities(
            fallback.candidate,
            ionosphere_available,
            ephemeris_available,
        )
    }
}

impl Default for GuiNavSourceStatus {
    fn default() -> Self {
        Self::iq_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nav_status_represents_iq_only_mode() {
        let status = GuiNavSourceStatus::iq_only();

        assert_eq!(status.kind, GuiNavSourceKind::IqOnly);
        assert_eq!(status.label, "IQ-only mode");
        assert!(status.path.is_none());
        assert!(!status.ionosphere_available);
        assert!(!status.ephemeris_available);
    }

    #[test]
    fn test_nav_status_maps_rinex_candidate_to_gui_status() {
        let status = GuiNavSourceStatus::from_rinex_candidate(
            RinexNavCandidate {
                path: PathBuf::from("data/brdc0940.13n"),
                source_kind: RinexNavSourceKind::Colocated,
                source_label: "rinex-nav-colocated".to_string(),
            },
            true,
        );

        assert_eq!(status.kind, GuiNavSourceKind::Colocated);
        assert_eq!(status.label, "Colocated RINEX nav");
        assert_eq!(status.path, Some(PathBuf::from("data/brdc0940.13n")));
        assert!(status.ionosphere_available);
        assert!(!status.ephemeris_available);
    }

    #[test]
    fn test_nav_status_maps_rinex_iono_fallback() {
        let status = GuiNavSourceStatus::from_rinex_iono_fallback(RinexIonoFallback {
            iono: gnss_core::nav::IonoParams {
                valid: true,
                ..Default::default()
            },
            candidate: RinexNavCandidate {
                path: PathBuf::from("manual.13n"),
                source_kind: RinexNavSourceKind::ExplicitPath,
                source_label: "rinex-nav-explicit".to_string(),
            },
        });

        assert_eq!(status.kind, GuiNavSourceKind::ExplicitPath);
        assert_eq!(status.path, Some(PathBuf::from("manual.13n")));
        assert!(status.ionosphere_available);
        assert!(!status.ephemeris_available);
    }

    #[test]
    fn test_nav_status_maps_rinex_nav_capabilities() {
        let mut catalog = gnss_core::nav::EphemerisCatalog::new();
        catalog.insert(
            1,
            vec![gnss_core::nav::Ephemeris {
                prn: 1,
                ..Default::default()
            }],
        );

        let status = GuiNavSourceStatus::from_rinex_nav_capabilities(RinexNavCapabilities {
            iono: Some(gnss_core::nav::IonoParams {
                valid: true,
                ..Default::default()
            }),
            ephemeris_catalog: catalog,
            candidate: RinexNavCandidate {
                path: PathBuf::from("manual.13n"),
                source_kind: RinexNavSourceKind::ExplicitPath,
                source_label: "rinex-nav-explicit".to_string(),
            },
        });

        assert_eq!(status.kind, GuiNavSourceKind::ExplicitPath);
        assert!(status.ionosphere_available);
        assert!(status.ephemeris_available);
    }
}
