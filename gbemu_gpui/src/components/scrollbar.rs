use core::time::Duration;
use std::{rc::Rc, time::Instant};

use gpui::{prelude::*, *};
use uzi::using;

use crate::{ext::ElementBoundsExt, theme::ThemeRegistry};

#[derive(IntoElement)]
pub struct Scrollbar {
    pub id: ElementId,
    pub offset: Pixels,
    pub max_offset: Pixels,
    pub on_drag_start: Option<Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>>,
    pub on_drag_end: Option<Rc<dyn Fn(&MouseUpEvent, &mut Window, &mut App)>>,
    pub on_drag_move: Option<Box<dyn Fn(&DragMoveEvent<()>, Point<Pixels>, &mut Window, &mut App)>>,
    dragging: bool,
}

impl Scrollbar {
    pub fn new(
        id: impl Into<ElementId>,
        offset: Pixels,
        max_offset: Pixels,
        dragging: bool,
    ) -> Self {
        Self {
            id: id.into(),
            offset,
            max_offset,
            on_drag_start: None,
            on_drag_end: None,
            on_drag_move: None,
            dragging,
        }
    }
}

impl RenderOnce for Scrollbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let viewport_bounds =
            window.use_keyed_state((self.id.clone(), "viewport_bounds"), cx, |_window, _cx| {
                Bounds::default()
            });

        let viewport_height = viewport_bounds.read(cx).size.height;

        let scrollbar_height = viewport_height / (self.max_offset + px(1.0)).pow(1.0 / 10.0);

        let scrollbar_offset =
            -self.offset / self.max_offset * (viewport_height - px(scrollbar_height));

        let hovered =
            window.use_keyed_state((self.id.clone(), "hovered"), cx, |_window, _cx| false);

        let last_hovered_time =
            window.use_keyed_state((self.id.clone(), "last_hover_time"), cx, |_window, _cx| {
                Instant::now() - Duration::from_hours(1)
            });

        let drag_start_position = window.use_keyed_state(
            (self.id.clone(), "drag_start_position"),
            cx,
            |_window, _cx| None,
        );

        let theme = cx.global::<ThemeRegistry>().current_theme();
        let border = theme.palette.gray();

        drop(theme);

        let dragging = self.dragging;
        if dragging {
            last_hovered_time.write(cx, Instant::now());
        }

        window.request_animation_frame();

        div()
            .size_full()
            .absolute()
            .top_0()
            .left_0()
            .child(
                div()
                    .block_mouse_except_scroll()
                    .id((self.id.clone(), "draggable"))
                    .opacity(if *hovered.read(cx) {
                        1.0
                    } else {
                        1.0 - (last_hovered_time
                            .read(cx)
                            .elapsed()
                            .saturating_sub(Duration::from_secs(1)))
                        .div_duration_f32(Duration::from_secs(1))
                        .clamp(0.0, 1.0)
                    })
                    .absolute()
                    .right_2()
                    .top(scrollbar_offset)
                    .h(px(scrollbar_height))
                    .bg(border)
                    .w_2()
                    .rounded_xl()
                    .on_drag((), |_, _, _, cx: &mut App| cx.new(|cx| EmptyView))
                    .on_mouse_down(
                        MouseButton::Left,
                        using!([drag_start_position], move |event, window, cx| {
                            drag_start_position.write(cx, Some(event.position));
                            if let Some(handler) = &self.on_drag_start {
                                handler(event, window, cx);
                            }
                        }),
                    )
                    .on_drag_move(using!([drag_start_position], move |event, window, cx| {
                        if let Some(handler) = &self.on_drag_move
                            && let Some(drag_origin) = drag_start_position.read(cx)
                        {
                            handler(event, *drag_origin, window, cx);
                        }
                    }))
                    .on_mouse_up_out(
                        MouseButton::Left,
                        using!(
                            [self.on_drag_end, drag_start_position],
                            move |event, window, cx| {
                                drag_start_position.write(cx, None);

                                if let Some(handler) = &on_drag_end {
                                    handler(event, window, cx);
                                }
                            }
                        ),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        using!([drag_start_position], move |event, window, cx| {
                            drag_start_position.write(cx, None);

                            if let Some(handler) = &self.on_drag_end {
                                handler(event, window, cx);
                            }
                        }),
                    ),
            )
            .child(
                div()
                    .id((self.id.clone(), "hover_box"))
                    .absolute()
                    .top_0()
                    .right_0()
                    .w_6()
                    .h_full()
                    .opacity(0.0)
                    .on_hover(using!(
                        [last_hovered_time, hovered],
                        move |hovering, _window, cx| {
                            if !*hovering {
                                last_hovered_time.write(cx, Instant::now());
                            }
                            hovered.write(cx, *hovering);
                        }
                    )),
            )
            .on_bounds_prepaint(using!([viewport_bounds], move |bounds, _window, cx| {
                viewport_bounds.write(cx, *bounds);
            }))
    }
}

#[derive(IntoElement)]
pub struct ListScrollbar {
    id: ElementId,
    list_state: ListState,
}

impl ListScrollbar {
    pub fn new(id: impl Into<ElementId>, list_state: ListState) -> Self {
        Self {
            id: id.into(),
            list_state,
        }
    }
}
impl RenderOnce for ListScrollbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let initial_offset =
            window.use_keyed_state((self.id.clone(), "initial_scroll_offset"), cx, |_, _| None);

        let mut sb = Scrollbar::new(
            self.id.clone(),
            self.list_state.scroll_px_offset_for_scrollbar().y,
            self.list_state.max_offset_for_scrollbar().y,
            initial_offset.read(cx).is_some(),
        );

        sb.on_drag_start = Some(Box::new(using!(
            [self.list_state, initial_offset],
            move |event, window, cx| {
                initial_offset.write(cx, Some(list_state.scroll_px_offset_for_scrollbar()));
                list_state.scrollbar_drag_started()
            }
        )));

        sb.on_drag_end = Some(Rc::new(using!(
            [self.list_state, initial_offset],
            move |event, window, cx| {
                initial_offset.write(cx, None);
                list_state.scrollbar_drag_ended()
            }
        )));

        sb.on_drag_move = Some(Box::new(using!(
            [self.list_state, initial_offset],
            move |event, drag_origin, window, cx| {
                if let Some(initial_scroll_offset) = initial_offset.read(cx) {
                    let offset = event.event.position - drag_origin;
                    list_state.set_offset_from_scrollbar(*initial_scroll_offset - offset);
                }
            }
        )));

        sb
    }
}
