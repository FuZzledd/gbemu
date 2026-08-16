use gpui::*;
use gpui::{prelude::FluentBuilder, red};

use crate::{PromptRenderer, theme::ThemeRegistry};

pub struct Root {
    pub view: AnyView,
    pub focus_handle: FocusHandle,
    pub on_close_request: Box<dyn FnMut(&mut Window, &mut App)>,
}

impl Root {
    pub fn new(view: impl Into<AnyView>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let entity = cx.new(|cx| {
            let elem = Self {
                focus_handle: cx.focus_handle(),
                view: view.into(),
                on_close_request: Box::new(|window, _cx| {
                    window.remove_window();
                }),
            };

            cx.subscribe_self(|this: &mut Root, _: &CloseRequestEvent, cx| {
                cx.with_window(cx.entity_id(), |window, cx| {
                    (this.on_close_request)(window, cx)
                })
                .unwrap();
            })
            .detach();

            if let Ok(entity) = elem.view.clone().downcast::<PromptRenderer>() {
                cx.subscribe(
                    &entity,
                    |this, entity, PromptResponse(response): &PromptResponse, cx| {
                        cx.emit(PromptResponse(*response));
                    },
                )
                .detach()
            }

            elem
        });

        window.on_window_should_close(cx, |window, cx| {
            window
                .root::<Root>()
                .unwrap()
                .unwrap()
                .update(cx, |_root, cx| {
                    cx.emit(CloseRequestEvent);
                });

            false
        });

        entity
    }

    pub fn on_close_request(&mut self, handler: impl FnMut(&mut Window, &mut App) + 'static) {
        self.on_close_request = Box::new(handler);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CloseRequestEvent;
impl EventEmitter<CloseRequestEvent> for Root {}

impl EventEmitter<PromptResponse> for Root {}

impl Focusable for Root {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if let Ok(prompt) = self.view.clone().downcast::<PromptRenderer>() {
            prompt.focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl Render for Root {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<'_, Root>,
    ) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.background();

        let border = theme.palette.gray();

        let tiling = match window.window_decorations() {
            Decorations::Server => Tiling::tiled(),
            Decorations::Client { tiling } => tiling,
        };

        div()
            .max_size_full()
            .size_full()
            .bg(gpui::transparent_black())
            .p_2()
            .when(tiling.top, |this| this.pt_0())
            .when(tiling.bottom, |this| this.pb_0())
            .when(tiling.left, |this| this.pl_0())
            .when(tiling.right, |this| this.pr_0())
            .flex()
            .justify_between()
            .child(
                div()
                    .when_else(
                        window.is_fullscreen(),
                        |this| this,
                        |this| {
                            this
                                // .bg(background)
                                .border_color(border)
                                .border_1()
                                .shadow_sm()
                                .rounded_sm()
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .right_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .h_full()
                                            .w_2()
                                            .cursor_e_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window.start_window_resize(ResizeEdge::Right);
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .left_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .h_full()
                                            .w_2()
                                            .cursor_w_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window.start_window_resize(ResizeEdge::Left);
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .top_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .w_full()
                                            .h_2()
                                            .cursor_n_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window.start_window_resize(ResizeEdge::Top);
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .bottom_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .w_full()
                                            .h_2()
                                            .cursor_s_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window.start_window_resize(ResizeEdge::Bottom);
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .top_neg_1()
                                            .right_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .w_3()
                                            .h_3()
                                            .cursor_nesw_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window
                                                        .start_window_resize(ResizeEdge::TopRight);
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .top_neg_1()
                                            .left_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .w_3()
                                            .h_3()
                                            .cursor_nwse_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window.start_window_resize(ResizeEdge::TopLeft);
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .bottom_neg_1()
                                            .right_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .w_3()
                                            .h_3()
                                            .cursor_nwse_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window.start_window_resize(
                                                        ResizeEdge::BottomRight,
                                                    );
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                                .child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .bottom_neg_1()
                                            .left_neg_1()
                                            .opacity(0.0)
                                            .bg(red())
                                            .w_3()
                                            .h_3()
                                            .cursor_nesw_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_event, window, cx| {
                                                    cx.stop_propagation();
                                                    window.start_window_resize(
                                                        ResizeEdge::BottomLeft,
                                                    );
                                                },
                                            ),
                                    )
                                    .priority(100),
                                )
                        },
                    )
                    .child(self.view.clone())
                    .flex_grow_1()
                    .flex()
                    .flex_col()
                    .overflow_hidden(),
            )
    }
}
