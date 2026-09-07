use gpui::*;

use crate::{
    WindowMap, WindowType, actions,
    components::{Root, TitleBar},
    theme::ThemeRegistry,
};

pub struct AboutWindow {
    focus: FocusHandle,
}

impl AboutWindow {
    pub fn open(
        _window: &mut Window,
        cx: &mut App,
    ) -> std::result::Result<gpui::WindowHandle<Root>, anyhow::Error> {
        let bounds = Bounds::centered(None, size(px(700.0), px(400.0)), cx);
        cx.open_window(
            WindowOptions {
                window_decorations: Some(WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: Some("Settings".into()),
                    ..Default::default()
                }),
                is_resizable: false,
                app_owns_titlebar_drag: true,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("uk.fuzzle.octopus".into()),
                window_min_size: Some(size(px(700.0), px(400.0))),
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
            cx.global_mut::<WindowMap>().remove(&WindowType::About);
        })
        .detach();

        entity
    }
}

impl Render for AboutWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.background();
        let foreground = theme.palette.foreground();
        let _border = theme.palette.gray();

        drop(theme);

        let element_id = ElementId::from(("About", cx.entity_id()));

        // let image = window
        //     .use_asset::<ImageAssetLoader>(&Resource::Embedded("assets/octopus-gb.svg".into()), cx);

        let date_string = {
            match jiff::Zoned::now().year() {
                2026 => "2026".into(),
                year => format!("2026–{year}"),
            }
        };

        div()
            .on_action::<actions::dev::ToggleInspector>(|_, window, cx| {
                #[cfg(debug_assertions)]
                window.toggle_inspector(cx);
            })
            .overflow_hidden()
            .track_focus(&self.focus)
            .size_full()
            .max_size_full()
            .flex()
            .flex_col()
            .flex_1()
            .items_stretch()
            .bg(background)
            .text_color(foreground)
            .child(
                TitleBar::new((element_id.clone(), "Titlebar"))
                    .flex()
                    .items_stretch()
                    .content_center()
                    .child(
                        div()
                            .flex_auto()
                            .flex()
                            .justify_center()
                            .items_center()
                            .child("About"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_stretch()
                    .flex_1()
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .h_full()
                            .max_h_full()
                            .p_6()
                            .overflow_hidden()
                            .justify_center()
                            .items_center()
                            .child(
                                img("assets/octopus-gb.png")
                                    .overflow_hidden()
                                    .h_56()
                                    .w_auto()
                                    .object_fit(ObjectFit::ScaleDown),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .items_stretch()
                            .text_sm()
                            .child(
                                div()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .text_align(TextAlign::Center)
                                    .text_size(rems(3.0))
                                    .child("Octopus-GB"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .text_align(TextAlign::Center)
                                    .child(format!("version: {}", env!("CARGO_PKG_VERSION"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .text_align(TextAlign::Center)
                                    .child(format!("branch: {}", env!("VERGEN_GIT_BRANCH"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .text_align(TextAlign::Center)
                                    .child(format!("commit: {}", env!("VERGEN_GIT_SHA"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .text_align(TextAlign::Center)
                                    .child(format!("debug: {}", env!("VERGEN_CARGO_DEBUG"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .text_align(TextAlign::Center)
                                    .child(format!(
                                        "rustc: {} {}",
                                        env!("VERGEN_RUSTC_SEMVER"),
                                        env!("VERGEN_RUSTC_HOST_TRIPLE")
                                    )),
                            )
                            .child(
                                div()
                                    .p_4()
                                    .mb_2()
                                    .flex()
                                    .flex_1()
                                    .flex_col()
                                    .justify_end()
                                    .items_center()
                                    .text_xs()
                                    .child(format!(
                                        "© {date_string} Zoey Freed, licensed under {license} ",
                                        license = env!("CARGO_PKG_LICENSE")
                                    ))
                                    .child(
                                        "Game Boy is a registered trademark of Nintendo Co., Ltd.",
                                    ),
                            ),
                    ),
            )
    }
}
