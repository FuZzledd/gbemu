use core::time::Duration;

use crate::{
    components::{
        button::Button,
        root::{CloseRequestEvent, Root},
    },
    theme::ThemeRegistry,
};
use gpui::{prelude::*, *};
use tap::Tap;
use uzi::using;

#[derive(IntoElement)]
pub struct TitleBar {
    id: ElementId,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl TitleBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: Default::default(),
            children: Default::default(),
        }
    }
}

impl ParentElement for TitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for TitleBar {
    #[doc = " Returns a reference to the style memory of this element."]
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.darkest_background();

        let foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        let red = theme.palette.red();

        drop(theme);

        let drag_task = window.use_keyed_state(self.id.clone(), cx, |_, _| None);

        div()
            .id(self.id.clone())
            .window_control_area(WindowControlArea::Drag)
            .w_full()
            .h_8()
            .bg(background)
            .border_color(border)
            .border_b_1()
            .text_color(foreground)
            .tap_mut(|div| div.style().refine(&self.style))
            .on_click(|event, window, _| {
                if event.standard_click() && event.click_count() == 2 {
                    window.zoom_window()
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                using!([drag_task], move |_, window, cx| {
                    let mut window = window.to_async(cx);
                    drag_task.write(
                        cx,
                        Some(cx.spawn(async move |cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(100))
                                .await;
                            window
                                .update(|window, _| window.start_window_move())
                                .unwrap();
                        })),
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                using!([drag_task], move |_, _, cx| {
                    drag_task.write(cx, None);
                }),
            )
            .on_mouse_down(MouseButton::Right, |event, window, _cx| {
                window.show_window_menu(event.position);
            })
            .children(self.children)
            .child(
                div()
                    .h_full()
                    .absolute()
                    .right_1()
                    .flex()
                    .gap_0p5()
                    .items_center()
                    .justify_center()
                    .child(
                        Button::new(
                            Duration::from_millis(300),
                            (self.id.clone(), "titlebar-minimize"),
                        )
                        .window_control_area(WindowControlArea::Max)
                        .on_any_mouse_down(|_event, _window, cx| {
                            cx.stop_propagation();
                        })
                        .on_click(|_event, window, cx| {
                            cx.stop_propagation();
                            window.minimize_window();
                        })
                        .flex()
                        .items_center()
                        .child(
                            svg()
                                .path("icons/window-minimize")
                                .size(relative(0.8))
                                .text_color(foreground),
                        )
                        .size_5()
                        .rounded_2xl()
                        .justify_center(),
                    )
                    .child(
                        Button::new(
                            Duration::from_millis(300),
                            (self.id.clone(), "titlebar-maximize"),
                        )
                        .window_control_area(WindowControlArea::Max)
                        .on_any_mouse_down(|_event, _window, cx| {
                            cx.stop_propagation();
                        })
                        .on_click(|_event, window, cx| {
                            cx.stop_propagation();
                            window.zoom_window();
                        })
                        .flex()
                        .items_center()
                        .child(
                            svg()
                                .path(if window.is_maximized() {
                                    "icons/window-restore"
                                } else {
                                    "icons/window-maximize"
                                })
                                .size(relative(0.8))
                                .text_color(foreground),
                        )
                        .size_5()
                        .rounded_2xl()
                        .justify_center(),
                    )
                    .child(
                        Button::new(
                            Duration::from_millis(300),
                            (self.id.clone(), "titlebar-close"),
                        )
                        .hover_background(red)
                        .window_control_area(WindowControlArea::Close)
                        .on_any_mouse_down(|_event, _window, cx| {
                            cx.stop_propagation();
                        })
                        .on_click(|_event, window, cx| {
                            cx.stop_propagation();
                            window
                                .root::<Root>()
                                .unwrap()
                                .unwrap()
                                .update(cx, |_root, cx| {
                                    cx.emit(CloseRequestEvent);
                                })
                        })
                        .flex()
                        .items_center()
                        .child(
                            svg()
                                .path("icons/window-close")
                                .size(relative(0.8))
                                .text_color(foreground),
                        )
                        .size_5()
                        .rounded_2xl()
                        .justify_center(),
                    ),
            )
    }
}
