//! Headless pixel capture of every GNSScope surface at every supported size.
//!
//! Renders the production `GnsscopeApp` off-screen through
//! `VisualTestAppContext` (real Metal rendering, texture-read capture) so the
//! interface can be reviewed and iterated without native screen capture. This
//! is the evidence harness for the interface-wide design pass: it covers all
//! four pages, both themes, both languages, and the 1568/1440/1100/800 width
//! matrix, writing append-only PNGs under the repo-root
//! `artifacts/ui-design-pass/` directory.
//!
//! The deterministic preview fixtures (`GNSSCOPE_PREVIEW=1`) supply the frozen
//! §3.0 story; the capture never touches the user's run library.
//!
//! This must run as a binary: the macOS platform is only created on the
//! process main thread, which the Rust test harness cannot provide.
//!
//! Usage:
//!   cargo run -p gnss-gui --example capture_surfaces -- workspaces
//!   cargo run -p gnss-gui --example capture_surfaces -- pages
//!   cargo run -p gnss-gui --example capture_surfaces -- matrix
//!   cargo run -p gnss-gui --example capture_surfaces -- all

use gnss_gui::GnsscopeApp;
use gpui_kit::{px, size, AppContext};
use std::sync::Arc;

const ARTIFACT_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../artifacts/ui-design-pass"
);

const REFERENCE_WIDTH: f32 = 1568.0;
const REFERENCE_HEIGHT: f32 = 1003.0;

/// Every UI surface the design pass must cover.
struct Surface {
    /// File suffix.
    name: &'static str,
    env: &'static [(&'static str, &'static str)],
}

const SURFACES: &[Surface] = &[
    Surface {
        name: "overview",
        env: &[("GNSSCOPE_WORKSPACE_VIEW", "overview")],
    },
    Surface {
        name: "channels",
        env: &[("GNSSCOPE_WORKSPACE_VIEW", "channels")],
    },
    Surface {
        name: "position",
        env: &[("GNSSCOPE_WORKSPACE_VIEW", "position")],
    },
    Surface {
        name: "navigation",
        env: &[("GNSSCOPE_WORKSPACE_VIEW", "navigation")],
    },
    Surface {
        name: "candidates",
        env: &[("GNSSCOPE_WORKSPACE_VIEW", "candidates")],
    },
    Surface {
        name: "new-analysis",
        env: &[("GNSSCOPE_OPEN_NEW_ANALYSIS", "1")],
    },
    Surface {
        name: "run-library",
        env: &[("GNSSCOPE_OPEN_RUN_LIBRARY", "1")],
    },
    Surface {
        name: "interface-settings",
        env: &[("GNSSCOPE_OPEN_INTERFACE_SETTINGS", "1")],
    },
    Surface {
        name: "receiver-settings",
        env: &[("GNSSCOPE_OPEN_SETTINGS", "1")],
    },
    Surface {
        name: "comparison",
        env: &[("GNSSCOPE_OPEN_COMPARISON", "1")],
    },
];

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    std::fs::create_dir_all(ARTIFACT_DIR).expect("create artifact directory");
    match mode.as_str() {
        "workspaces" => capture_workspaces(),
        "pages" => capture_pages(),
        "matrix" => capture_matrix(),
        "all" => {
            capture_workspaces();
            capture_pages();
            capture_matrix();
        }
        other => panic!("unknown mode {other}; use workspaces|pages|matrix|all"),
    }
}

/// Both themes for every run-local workbench view at the reference canvas.
fn capture_workspaces() {
    for surface in &SURFACES[..5] {
        for theme in ["dark", "light"] {
            let out = format!("{ARTIFACT_DIR}/surface-{}-{theme}.png", surface.name);
            let image = capture(surface.env, theme, "zh", REFERENCE_WIDTH, REFERENCE_HEIGHT);
            image.save(&out).expect("save capture");
            println!("captured {out}");
        }
    }
}

/// Both themes for every non-workbench page at the reference canvas.
fn capture_pages() {
    for surface in &SURFACES[5..] {
        for theme in ["dark", "light"] {
            let out = format!("{ARTIFACT_DIR}/surface-{}-{theme}.png", surface.name);
            let image = capture(surface.env, theme, "zh", REFERENCE_WIDTH, REFERENCE_HEIGHT);
            image.save(&out).expect("save capture");
            println!("captured {out}");
        }
    }
}

/// Overview across the required width matrix in both themes and languages.
fn capture_matrix() {
    for (width, height) in [
        (1568.0, 1003.0),
        (1440.0, 900.0),
        (1100.0, 800.0),
        (800.0, 720.0),
    ] {
        for theme in ["dark", "light"] {
            for language in ["zh", "en"] {
                let out = format!(
                    "{ARTIFACT_DIR}/matrix-{}-{theme}-{language}-{}x{}.png",
                    "overview", width as u32, height as u32
                );
                let image = capture(
                    &[("GNSSCOPE_WORKSPACE_VIEW", "overview")],
                    theme,
                    language,
                    width,
                    height,
                );
                image.save(&out).expect("save capture");
                println!("captured {out}");
            }
        }
    }
}

fn capture(
    env: &[(&str, &str)],
    theme: &str,
    language: &str,
    width: f32,
    height: f32,
) -> image::RgbaImage {
    std::env::set_var("GNSSCOPE_PREVIEW", "1");
    std::env::set_var("GNSSCOPE_THEME", theme);
    std::env::set_var("GNSSCOPE_LANGUAGE", language);
    for (key, value) in env {
        std::env::set_var(key, value);
    }

    let platform = gpui_kit::platform::current_platform(true);
    let mut cx = gpui_kit::VisualTestAppContext::with_asset_source(
        platform,
        Arc::new(gpui_kit::assets::Assets),
    );
    cx.update(gpui_kit::init);
    let window = cx
        .open_offscreen_window(size(px(width), px(height)), |window, cx| {
            let app = cx.new(|cx| GnsscopeApp::new(window, cx));
            cx.new(|cx| gpui_kit::component::Root::new(app, window, cx))
        })
        .unwrap_or_else(|error| {
            panic!("{theme}/{language}/{width}x{height} window failed: {error}")
        });
    cx.run_until_parked();
    cx.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();

    let image = cx
        .capture_screenshot(window.into())
        .unwrap_or_else(|error| panic!("{theme}/{width}x{height} capture failed: {error}"));

    std::env::remove_var("GNSSCOPE_PREVIEW");
    std::env::remove_var("GNSSCOPE_THEME");
    std::env::remove_var("GNSSCOPE_LANGUAGE");
    for (key, _) in env {
        std::env::remove_var(key);
    }

    // The off-screen window is created at the requested logical size; the
    // backing scale factor follows the host display, so verify the canvas
    // aspect and a sane scale rather than pinning one exact factor.
    let (captured_width, captured_height) = image.dimensions();
    let scale_x = f64::from(captured_width) / f64::from(width);
    let scale_y = f64::from(captured_height) / f64::from(height);
    // The backing scale follows the host display (2x on a default Retina mode,
    // ~1.91 in a scaled mode) and integer rounding differs per axis, so accept
    // a small scale mismatch rather than pinning one exact factor.
    assert!(
        scale_x >= 1.0 && (scale_x - scale_y).abs() < 0.03,
        "unexpected capture dimensions {captured_width}x{captured_height} for {width}x{height}"
    );
    assert!(
        !is_uniform(&image),
        "capture for {width}x{height} must not be a flat frame"
    );
    image
}

fn is_uniform(image: &image::RgbaImage) -> bool {
    let mut distinct = std::collections::HashSet::new();
    for pixel in image.pixels() {
        distinct.insert(pixel.0);
        if distinct.len() > 64 {
            return false;
        }
    }
    true
}
