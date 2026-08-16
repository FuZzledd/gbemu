use core::time::Duration;
use std::borrow::Cow;

use gpui::{prelude::*, *};
use itertools::Itertools;
use tap::Tap;
use uzi::using;

use crate::{
    components::{button::Button, scrollbar::ListScrollbar},
    ext::ElementBoundsExt,
    theme::ThemeRegistry,
};

type DropdownOption<T, K = Cow<'static, str>> = (K, T);

pub struct Dropdown<T, K = Cow<'static, str>> {
    pub style: StyleRefinement,
    pub options: Vec<DropdownOption<T, K>>,
    pub selected_idx: usize,
    bounds: Option<Bounds<Pixels>>,
    list_state: ListState,
    open: bool,
    focus_handle: FocusHandle,
}

impl<T: 'static, K: 'static> Dropdown<T, K> {
    pub fn new<Key: Into<K>>(
        options: impl IntoIterator<Item = DropdownOption<T, Key>>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| Self::new_raw(options, cx))
    }

    pub fn new_raw<Key: Into<K>>(
        options: impl IntoIterator<Item = DropdownOption<T, Key>>,
        cx: &mut App,
    ) -> Self {
        let options: Vec<(K, T)> = options.into_iter().map(|(k, v)| (k.into(), v)).collect();
        Dropdown {
            style: StyleRefinement::default(),
            selected_idx: 0,
            bounds: None,
            list_state: ListState::new(options.len(), ListAlignment::Top, px(25.0)).measure_all(),
            options,
            open: false,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn get_selected(&self) -> &DropdownOption<T, K> {
        &self.options[self.selected_idx]
    }
}

impl<T: Clone + 'static> Render for Dropdown<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("Dropdown", entity_id));
        let self_entity = cx.entity();

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.darker_background();
        let lighter_background = theme.palette.lighter_background();

        let foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        drop(theme);

        Button::new(Duration::from_millis(200), (element_id.clone(), "button"))
            .rounded_md()
            .border_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .items_stretch()
                    .h_full()
                    .child(
                        div()
                            .px_2()
                            .flex()
                            .justify_start()
                            .items_baseline()
                            .child(self.options[self.selected_idx].0.clone())
                            .flex_1(),
                    )
                    .child(
                        div()
                            .border_l_1()
                            .border_color(border)
                            .flex()
                            .justify_center()
                            .items_center()
                            .child(
                                svg()
                                    .path("icons/chevron-down")
                                    .size_6()
                                    .aspect_square()
                                    .text_color(foreground),
                            )
                            .px_1()
                            .h_full()
                            .flex_none(),
                    ),
            )
            .track_focus(&self.focus_handle)
            .on_bounds_prepaint(cx.listener(|this, bounds: &Bounds<Pixels>, window, cx| {
                this.bounds = Some(bounds.clone());
            }))
            .when(self.open, |this| {
                this.when_some(self.bounds, |this, bounds| {
                    this.child(deferred(
                        anchored()
                            .position_mode(AnchoredPositionMode::Window)
                            .snap_to_window_with_margin(px(16.0))
                            .anchor(Anchor::TopLeft)
                            .position(bounds.bottom_left())
                            .child(DropdownPopup::new(self_entity.downgrade())),
                    ))
                })
                .on_key_down(cx.listener(
                    |this, event: &KeyDownEvent, window, cx| {
                        if let Some(char) = &event.keystroke.key_char
                            && let Some((idx, _)) = this
                                .options
                                .iter()
                                .find_position(|item| item.0.to_lowercase().starts_with(char))
                        {
                            this.list_state.scroll_to_reveal_item(idx);
                        }
                    },
                ))
            })
            .on_click(cx.listener(|this, event, window, cx| {
                this.open = true;
            }))
            .tap_mut(|this| this.style().refine(&self.style))
    }
}

impl<T: 'static> EventEmitter<DismissEvent> for Dropdown<T> {}

#[derive(IntoElement)]
struct DropdownPopup<T: Clone + 'static> {
    parent: WeakEntity<Dropdown<T>>,
    style: StyleRefinement,
}

impl<T: Clone> Styled for DropdownPopup<T> {
    #[doc = " Returns a reference to the style memory of this element."]
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone> DropdownPopup<T> {
    fn new(parent: WeakEntity<Dropdown<T>>) -> Self {
        Self {
            parent,
            style: StyleRefinement::default(),
        }
    }
}

impl<T: Clone + 'static> RenderOnce for DropdownPopup<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let parent = self.parent.upgrade().expect("Couldn't get parent entity");

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.darker_background();
        let lighter_background = theme.palette.lighter_background();

        let _hover_background = theme.palette.background();

        let _foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        drop(theme);

        let element_id = ElementId::from(("DropdownPopup", parent.entity_id()));

        let Dropdown {
            options,
            selected_idx,
            bounds,
            list_state,
            ..
        } = parent.read(cx);
        let list_state = list_state.clone();
        let options = options.clone();
        let bounds = bounds.unwrap();
        let selected_idx = *selected_idx;

        div()
            .bg(background)
            .block_mouse_except_scroll()
            .child(
                list(
                    list_state.clone(),
                    using!([element_id, parent], move |idx, window, cx| {
                        let option = options[idx].clone();

                        Button::new(
                            Duration::from_millis(100),
                            (element_id.clone(), format!("dropdown_item {idx}").as_str()),
                        )
                        .when(idx == selected_idx, |this| {
                            this.background(lighter_background)
                        })
                        .child(option.0)
                        .on_click(using!([parent], move |event, window, cx| {
                            parent.update(cx, move |parent, cx| {
                                parent.selected_idx = idx;
                                parent.open = false;
                                cx.notify();
                            })
                        }))
                        .px_1()
                        .w(bounds.size.width)
                        .into_any_element()
                    }),
                )
                .flex()
                .flex_col()
                .items_stretch()
                .with_sizing_behavior(ListSizingBehavior::Infer)
                .w_full()
                .max_h(0.75 * (window.viewport_size().height - bounds.bottom()))
                .bg(background),
            )
            .child(ListScrollbar::new(
                (element_id.clone(), "scrollbar"),
                list_state,
            ))
            .border_color(border)
            .border_1()
            .rounded_sm()
            .flex_basis(px(0.0))
            .flex_col()
            .w(bounds.size.width)
            .on_mouse_down_out(using!([parent], move |event, window, cx| {
                parent.update(cx, |parent, cx| parent.open = false)
            }))
            .tap_mut(|this| this.style().refine(&self.style))
    }
}
