use std::{io::Cursor, sync::Arc};

use gpui::*;

use crate::components::{Root, TitleBar};
use crate::{WindowMap, WindowType};

pub struct LunaWindow {
    focus: FocusHandle,
}

impl LunaWindow {
    pub fn open(
        _window: &mut Window,
        cx: &mut App,
    ) -> std::result::Result<gpui::WindowHandle<Root>, anyhow::Error> {
        let bounds = Bounds::centered(None, size(px(600.0), px(700.0)), cx);
        cx.open_window(
            WindowOptions {
                window_decorations: Some(WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: Some("Luna".into()),
                    ..Default::default()
                }),
                is_resizable: false,
                app_owns_titlebar_drag: true,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("uk.fuzzle.luna".into()),
                icon: Some(Arc::new(
                    image::ImageReader::with_format(
                        Cursor::new(
                            cx.asset_source()
                                .load("assets/luna-octopus.png")
                                .unwrap()
                                .unwrap(),
                        ),
                        image::ImageFormat::Png,
                    )
                    .decode()
                    .unwrap()
                    .into_rgba8(),
                )),
                window_min_size: Some(size(px(600.0), px(700.0))),
                ..Default::default()
            },
            Self::create_root,
        )
    }
    pub fn create_root(window: &mut Window, cx: &mut App) -> Entity<Root> {
        let about_window = Self::new(window, cx);
        let root = Root::new(about_window.clone(), window, cx);
        root.update(cx, |root, _cx| {
            root.on_close_request(move |window, _cx| window.remove_window())
        });
        root
    }

    pub fn new(_window: &mut Window, cx: &mut App) -> Entity<Self> {
        let entity = cx.new(|cx| Self {
            focus: cx.focus_handle(),
        });

        cx.observe_release(&entity, |_this, cx| {
            cx.global_mut::<WindowMap>().remove(&WindowType::Luna);
        })
        .detach();

        entity
    }
}

impl Render for LunaWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let element_id = ElementId::from(("Luna", cx.entity_id()));

        div()
            .flex()
            .flex_col()
            .size_full()
            .items_stretch()
            .child(
                TitleBar::new(element_id)
                    .flex()
                    .items_stretch()
                    .content_center()
                    .child(
                        div()
                            .flex_auto()
                            .flex()
                            .justify_center()
                            .items_center()
                            .child("Luna"),
                    ),
            )
            .child(
                div().flex().flex_1().items_center().justify_center().child(
                    img("assets/luna-octopus.png")
                        .object_fit(ObjectFit::ScaleDown)
                        .h_4_5()
                        .w_4_5(),
                ),
            )
    }
}
