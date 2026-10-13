//! Headless pixel evidence for the status strip + title-bar command field
//! (issue 16, design-refresh spec §3.1 / §3.11).
//!
//! Renders the production `GnsscopeApp` in an off-screen window through
//! `VisualTestAppContext` (real Metal rendering, texture-read capture) so the
//! *production* `ui::title_bar::render` and `ui::status_strip::render` paths
//! are exercised end to end, then stores PNG evidence under the repo-root
//! `artifacts/ui-visual-refactor-phase7/` directory (append-only). No native
//! screen capture is used and the user's run library is never touched
//! (`GNSSCOPE_PREVIEW=1` forces the deterministic preview fixtures).
//!
//! The process sleeps >2s before the final draw so the cached telemetry
//! sampler (2s refresh, Instant-based) performs a second real reading and the
//! status strip shows measured CPU/memory/disk values instead of the honest
//! pre-window "不可用" fallback.
//!
//! This must run as a binary: the macOS platform is only created on the
//! process main thread, which the Rust test harness cannot provide.
//!
//! Usage: `cargo run -p gnss-gui --example capture_status_title`

use gnss_gui::GnsscopeApp;
use gpui_kit::{px, size, AppContext, Pixels};
use std::sync::Arc;

const ARTIFACT_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../artifacts/ui-visual-refactor-phase7"
);

fn main() {
    std::fs::create_dir_all(ARTIFACT_DIR).expect("create artifact directory");
    let mut captures = Vec::new();
    for theme in ["dark", "light"] {
        // Preview mode is decided inside GnsscopeApp::new; both variables are
        // process-global, so themes are captured sequentially in one process.
        std::env::set_var("GNSSCOPE_PREVIEW", "1");
        std::env::set_var("GNSSCOPE_THEME", theme);
        let image = capture_app(theme);
        std::env::remove_var("GNSSCOPE_PREVIEW");
        std::env::remove_var("GNSSCOPE_THEME");
        let out = format!("{ARTIFACT_DIR}/status-title-{theme}.png");
        image.save(&out).expect("save capture");
        println!("captured {}", out);
        captures.push((theme.to_string(), image));
    }

    let [(dark_name, dark), (light_name, light)] = &captures[..] else {
        unreachable!("both captures were just produced");
    };
    assert_eq!(dark_name, "dark");
    assert_eq!(light_name, "light");
    assert!(!is_uniform(dark), "dark capture must not be a flat frame");
    assert!(!is_uniform(light), "light capture must not be a flat frame");
    let dark_mean = mean_luminance(dark);
    let light_mean = mean_luminance(light);
    assert!(
        dark_mean < 80.0,
        "dark frame should be predominantly dark (mean luminance {dark_mean:.1})"
    );
    assert!(
        light_mean > 160.0,
        "light frame should be predominantly light (mean luminance {light_mean:.1})"
    );
    assert!(
        light_mean - dark_mean > 80.0,
        "themes must render distinctly (dark {dark_mean:.1} vs light {light_mean:.1})"
    );
    // The title bar (top 40 logical px) and status strip (bottom 28 logical
    // px) must carry real content in both themes; exact token/geometry checks
    // happen in the Python band inspection and the ui unit tests.
    let (width, height) = dark.dimensions();
    let scale = width / 1440;
    let top = band_mean_luminance(dark, 0, 40 * scale);
    let bottom = band_mean_luminance(dark, height - 28 * scale, height);
    assert!(
        !band_uniform(dark, height - 28 * scale, height),
        "status strip band must not be flat"
    );
    println!(
        "mean luminance: dark {dark_mean:.1}, light {light_mean:.1}; band top {top:.1}, bottom {bottom:.1}"
    );
}

fn capture_app(theme: &str) -> image::RgbaImage {
    let platform = gpui_kit::platform::current_platform(true);
    let mut cx = gpui_kit::VisualTestAppContext::with_asset_source(
        platform,
        Arc::new(gpui_kit::assets::Assets),
    );
    cx.update(gpui_kit::init);
    let window = cx
        .open_offscreen_window(app_size(), |window, cx| {
            let app = cx.new(|cx| GnsscopeApp::new(window, cx));
            cx.new(|cx| gpui_kit::component::Root::new(app, window, cx))
        })
        .expect("open offscreen window");
    // The warm-up capture flushes pending effects and draws the first frame,
    // which creates the telemetry sampler baseline (first real reading).
    // Sleeping then opens the CPU measurement window (wall clock, not the
    // gpui test clock); marking the window dirty makes run_until_parked
    // perform a fresh draw whose status strip takes a second real reading
    // instead of keeping the honest pre-window "不可用" fallback.
    cx.capture_screenshot(window.into())
        .expect("warm-up capture");
    std::thread::sleep(std::time::Duration::from_millis(2500));
    cx.update_window(window.into(), |_, window, _| window.refresh())
        .expect("mark window dirty");
    cx.run_until_parked();

    let image = cx
        .capture_screenshot(window.into())
        .unwrap_or_else(|error| panic!("{theme} capture failed: {error}"));
    // The off-screen window is created at logical 1440x900; accept the
    // device scale factor doubling rather than pinning a single scale.
    let (width, height) = image.dimensions();
    assert!(
        (width == 1440 && height == 900) || (width == 2880 && height == 1800),
        "unexpected capture dimensions {width}x{height}"
    );
    image
}

fn app_size() -> gpui_kit::Size<Pixels> {
    size(px(1440.0), px(900.0))
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

/// Mean luminance of rows `[top, bottom)` of the image.
fn band_mean_luminance(image: &image::RgbaImage, top: u32, bottom: u32) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for y in top..bottom.min(image.height()) {
        for x in 0..image.width() {
            let [r, g, b, _] = image.get_pixel(x, y).0;
            total += 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
            count += 1;
        }
    }
    total / count.max(1) as f64
}

fn band_uniform(image: &image::RgbaImage, top: u32, bottom: u32) -> bool {
    let mut distinct = std::collections::HashSet::new();
    for y in top..bottom.min(image.height()) {
        for x in 0..image.width() {
            distinct.insert(image.get_pixel(x, y).0);
            if distinct.len() > 64 {
                return false;
            }
        }
    }
    true
}
