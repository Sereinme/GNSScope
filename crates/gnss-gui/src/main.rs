use gnss_gui::GnsscopeApp;
use gpui::{point, px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use gpui_component::{Root, TitleBar};
use gpui_component_assets::Assets;

pub fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(80.), px(60.)),
                    size: size(px(1440.), px(900.)),
                })),
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            },
            |window, cx| {
                let app = cx.new(GnsscopeApp::new);
                cx.new(|cx| Root::new(app, window, cx))
            },
        )
        .expect("open GNSScope window");
        cx.activate(true);
    });
}
