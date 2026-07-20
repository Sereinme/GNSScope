use gnss_gui::GnsscopeApp;
use gpui::{point, px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use gpui_component::{Root, TitleBar};
use gpui_component_assets::Assets;

fn configured_window_size() -> (f32, f32) {
    fn dimension(name: &str, fallback: f32) -> f32 {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 480.0)
            .unwrap_or(fallback)
    }
    (
        dimension("GNSSCOPE_WINDOW_WIDTH", 1440.0),
        dimension("GNSSCOPE_WINDOW_HEIGHT", 900.0),
    )
}

pub fn main() {
    let (window_width, window_height) = configured_window_size();
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(80.), px(60.)),
                        size: size(px(window_width), px(window_height)),
                    })),
                    titlebar: Some(TitleBar::title_bar_options()),
                    ..Default::default()
                },
                |window, cx| {
                    let app = cx.new(|cx| GnsscopeApp::new(window, cx));
                    cx.new(|cx| Root::new(app, window, cx))
                },
            )
            .expect("open GNSScope window");
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::configured_window_size;

    #[test]
    fn default_window_size_is_desktop_friendly() {
        std::env::remove_var("GNSSCOPE_WINDOW_WIDTH");
        std::env::remove_var("GNSSCOPE_WINDOW_HEIGHT");
        assert_eq!(configured_window_size(), (1440.0, 900.0));
    }
}
