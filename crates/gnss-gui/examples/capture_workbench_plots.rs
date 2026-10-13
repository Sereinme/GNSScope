//! Headless pixel capture of the workbench plot panes (issue 18).
//!
//! Renders the production `GnsscopeApp` in an off-screen window at the
//! design-refresh reference canvas (1568x1003, spec §3) through
//! `VisualTestAppContext` (real Metal rendering, texture-read capture) and
//! stores PNG evidence under the repo-root
//! `artifacts/ui-visual-refactor-phase7/` directory with a `-plots` suffix.
//! The deterministic preview fixtures (`GNSSCOPE_PREVIEW=1`) tell the §3.0
//! story: nine tracked channels feed the nine-lane C/N0 grid, seven of them
//! navigation-locked drive the sky-plot PVT rings. The capture never touches
//! the user's run library and never uses native screen capture.
//!
//! This must run as a binary: the macOS platform is only created on the
//! process main thread, which the Rust test harness cannot provide.
//!
//! Usage: `cargo run -p gnss-gui --example capture_workbench_plots`

use gnss_gui::GnsscopeApp;
use gpui_kit::{px, size, AppContext, Pixels};
use std::sync::Arc;

const ARTIFACT_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../artifacts/ui-visual-refactor-phase7"
);

/// Reference canvas of design-refresh spec §3 (1568x1003).
const REFERENCE_WIDTH: f32 = 1568.0;
const REFERENCE_HEIGHT: f32 = 1003.0;

fn main() {
    std::fs::create_dir_all(ARTIFACT_DIR).expect("create artifact directory");
    let mut captures = Vec::new();
    // Overview shows sky + ENU + C/N0 panes together; channels view gives the
    // C/N0 history pane full width for the nine-lane pixel check.
    for (view, view_suffix) in [("overview", ""), ("channels", "-channels")] {
        for theme in ["dark", "light"] {
            std::env::set_var("GNSSCOPE_PREVIEW", "1");
            std::env::set_var("GNSSCOPE_THEME", theme);
            std::env::set_var("GNSSCOPE_WORKSPACE_VIEW", view);
            let image = capture_reference_pane(theme, view);
            std::env::remove_var("GNSSCOPE_PREVIEW");
            std::env::remove_var("GNSSCOPE_THEME");
            std::env::remove_var("GNSSCOPE_WORKSPACE_VIEW");
            let out = format!("{ARTIFACT_DIR}/workbench-plots{view_suffix}-{theme}.png");
            image.save(&out).expect("save capture");
            println!("captured {}", out);
            captures.push((theme.to_string(), image));
        }
    }

    for (theme, image) in &captures {
        assert!(
            !is_uniform(image),
            "{theme} capture must not be a flat frame"
        );
        let mean = mean_luminance(image);
        if theme == "dark" {
            assert!(
                mean < 80.0,
                "dark reference pane should be predominantly dark (mean luminance {mean:.1})"
            );
        } else {
            assert!(
                mean > 160.0,
                "light reference pane should be predominantly light (mean luminance {mean:.1})"
            );
        }
    }
    let dark_mean = mean_luminance(&captures[0].1);
    let light_mean = mean_luminance(&captures[1].1);
    assert!(
        light_mean - dark_mean > 80.0,
        "themes must render distinctly (dark {dark_mean:.1} vs light {light_mean:.1})"
    );
    println!(
        "mean luminance: dark {dark_mean:.1}, light {light_mean:.1}; all captures carry real pixels"
    );
}

fn capture_reference_pane(theme: &str, view: &str) -> image::RgbaImage {
    let platform = gpui_kit::platform::current_platform(true);
    let mut cx = gpui_kit::VisualTestAppContext::with_asset_source(
        platform,
        Arc::new(gpui_kit::assets::Assets),
    );
    cx.update(gpui_kit::init);
    let window = cx
        .open_offscreen_window(reference_size(), |window, cx| {
            let app = cx.new(|cx| GnsscopeApp::new(window, cx));
            cx.new(|cx| gpui_kit::component::Root::new(app, window, cx))
        })
        .unwrap_or_else(|error| panic!("{theme}/{view} window failed: {error}"));
    // Let scheduled draws, timers, and preview fixtures settle before capture.
    cx.run_until_parked();
    cx.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();

    let image = cx
        .capture_screenshot(window.into())
        .unwrap_or_else(|error| panic!("{theme}/{view} capture failed: {error}"));
    // The off-screen window is created at logical 1568x1003; accept the
    // device scale factor doubling rather than pinning a single scale.
    let (width, height) = image.dimensions();
    assert!(
        (width == 1568 && height == 1003) || (width == 3136 && height == 2006),
        "unexpected capture dimensions {width}x{height}"
    );
    image
}

fn reference_size() -> gpui_kit::Size<Pixels> {
    size(px(REFERENCE_WIDTH), px(REFERENCE_HEIGHT))
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

fn mean_luminance(image: &image::RgbaImage) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for pixel in image.pixels() {
        let [r, g, b, _] = pixel.0;
        total += 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
        count += 1;
    }
    total / count.max(1) as f64
}
