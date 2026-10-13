//! Headless window-matrix capture of the shell-fixed epoch timeline.
//!
//! Re-captures the phase-6 timeline evidence AFTER the `workspace_shell`
//! re-parenting (timeline spans only the workbench column; the side region —
//! run-configuration rail or drawer — stays full height). The pre-fix
//! `timeline-*.png` files are kept untouched as the before/after comparison;
//! this example only writes `timeline-shellfix-*.png` files.
//!
//! Renders the production `GnsscopeApp` in off-screen windows through
//! `VisualTestAppContext` (real Metal rendering, texture-read capture). It
//! never uses native screen capture and never touches the user's run library
//! (`GNSSCOPE_PREVIEW=1` forces the deterministic preview fixtures).
//! `GNSSCOPE_THEME` and `GNSSCOPE_LANGUAGE` are process-level, so every
//! theme/language combination is captured sequentially in its own pass.
//!
//! This must run as a binary: the macOS platform is only created on the
//! process main thread, which the Rust test harness cannot provide.
//!
//! Usage: `cargo run -p gnss-gui --example capture_timeline_matrix`

use gnss_gui::GnsscopeApp;
use gpui_kit::{px, size, AppContext};
use std::sync::Arc;

const ARTIFACT_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../artifacts/ui-visual-refactor-phase6"
);

/// Window logical sizes from issue 13's acceptance matrix.
const MATRIX: [(u32, u32); 4] = [(1568, 1003), (1440, 900), (1100, 800), (800, 720)];

/// `(file label, GNSSCOPE_THEME, GNSSCOPE_LANGUAGE)`.
const COMBINATIONS: [(&str, &str, &str); 2] =
    [("dark-zh", "dark", "zh-CN"), ("light-en", "light", "en")];

fn main() {
    std::fs::create_dir_all(ARTIFACT_DIR).expect("create artifact directory");
    let mut results = Vec::new();
    for &(combo, theme, language) in &COMBINATIONS {
        for &(width, height) in &MATRIX {
            let image = capture(width, height, theme, language);
            let file = format!("{ARTIFACT_DIR}/timeline-shellfix-{width}x{height}-{combo}.png");
            image.save(&file).expect("save capture");
            println!("captured {file}");
            results.push((
                combo.to_string(),
                width,
                height,
                mean_luminance(&image),
                is_uniform(&image),
            ));
        }
    }

    assert_eq!(results.len(), MATRIX.len() * COMBINATIONS.len());
    for (combo, width, height, luminance, uniform) in &results {
        assert!(
            !uniform,
            "{combo} {width}x{height} capture must not be a flat frame"
        );
        if combo.starts_with("dark") {
            assert!(
                *luminance < 80.0,
                "dark {width}x{height} should be predominantly dark (mean luminance {luminance:.1})"
            );
        } else {
            assert!(
                *luminance > 160.0,
                "light {width}x{height} should be predominantly light (mean luminance {luminance:.1})"
            );
        }
    }
    for ((dark_combo, .., dark_luminance, _), (light_combo, .., light_luminance, _)) in
        results.iter().zip(results.iter().skip(results.len() / 2))
    {
        assert!(dark_combo.starts_with("dark") && light_combo.starts_with("light"));
        assert!(
            light_luminance - dark_luminance > 80.0,
            "themes must render distinctly ({dark_combo} {dark_luminance:.1} vs {light_combo} {light_luminance:.1})"
        );
    }
    println!(
        "all {} captures carry real, distinctly themed pixels",
        results.len()
    );
}

fn capture(width: u32, height: u32, theme: &str, language: &str) -> image::RgbaImage {
    // All three variables are read inside `GnsscopeApp::new`, so each capture
    // pass sets them before building its own off-screen context.
    std::env::set_var("GNSSCOPE_PREVIEW", "1");
    std::env::set_var("GNSSCOPE_THEME", theme);
    std::env::set_var("GNSSCOPE_LANGUAGE", language);

    let platform = gpui_kit::platform::current_platform(true);
    let mut cx = gpui_kit::VisualTestAppContext::with_asset_source(
        platform,
        Arc::new(gpui_kit::assets::Assets),
    );
    cx.update(gpui_kit::init);
    let window = cx
        .open_offscreen_window(size(px(width as f32), px(height as f32)), |window, cx| {
            let app = cx.new(|cx| GnsscopeApp::new(window, cx));
            cx.new(|cx| gpui_kit::component::Root::new(app, window, cx))
        })
        .unwrap_or_else(|error| panic!("open offscreen window {width}x{height}: {error}"));
    // Let scheduled draws, timers, and preview fixtures settle before capture.
    cx.run_until_parked();
    cx.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();

    let image = cx
        .capture_screenshot(window.into())
        .unwrap_or_else(|error| panic!("{theme} {width}x{height} capture failed: {error}"));

    std::env::remove_var("GNSSCOPE_PREVIEW");
    std::env::remove_var("GNSSCOPE_THEME");
    std::env::remove_var("GNSSCOPE_LANGUAGE");

    // The off-screen window is created at the requested logical size; accept
    // the device scale factor doubling rather than pinning a single scale.
    let (captured_width, captured_height) = image.dimensions();
    assert!(
        (captured_width == width && captured_height == height)
            || (captured_width == width * 2 && captured_height == height * 2),
        "unexpected capture dimensions {captured_width}x{captured_height} for {width}x{height}"
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
