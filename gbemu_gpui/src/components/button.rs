use core::time::Duration;
use std::time::Instant;

use gpui::Lerp;
use gpui::{prelude::*, *};
use tap::Tap;

use crate::{components::DefaultableInto, theme::ThemeRegistry};

#[derive(IntoElement)]
pub struct Button {
    base: Div,
    hover_progress: f32,
    hovered: bool,
    anim_start_time: Instant,
    anim_duration: Duration,
    background: Option<Rgba>,
    hover_background: Option<Rgba>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    id: ElementId,
    disabled: bool,
}
impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Button {
    pub fn new(hover_anim_duration: Duration, id: impl Into<ElementId>) -> Self {
        Button {
            background: None,
            hover_background: None,
            base: div(),
            hover_progress: 0.0,
            hovered: false,
            anim_start_time: Instant::now() - hover_anim_duration,
            anim_duration: hover_anim_duration,
            style: StyleRefinement::default(),
            children: vec![],
            id: id.into(),
            disabled: false,
        }
    }

    pub fn hover_background<T>(mut self, color: impl DefaultableInto<Rgba, T>) -> Self {
        self.hover_background = color.defaultable_into();
        self
    }

    pub fn background<T>(mut self, color: impl DefaultableInto<Rgba, T>) -> Self {
        self.background = color.defaultable_into();
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

impl Styled for Button {
    #[doc = " Returns a reference to the style memory of this element."]
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

struct ButtonInner {
    hover_progress: f32,
    hovered: bool,
    anim_start_time: Instant,
    anim_duration: Duration,
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Button {
            base,
            hover_progress,
            hovered,
            anim_start_time,
            anim_duration,
            hover_background,
            background,
            style,
            children,
            id,
            disabled,
        } = self;

        let state = window.use_keyed_state((id, "state"), cx, |_window, _cx| ButtonInner {
            hover_progress,
            hovered,
            anim_start_time,
            anim_duration,
        });

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = background.unwrap_or(theme.palette.darker_background().into());
        let hover_background = hover_background.unwrap_or(theme.palette.gray().into());
        let active_background: Rgba = theme.palette.white().into();
        let text_color = theme.palette.foreground();
        let disabled_text_color = theme.palette.dark_foreground();
        let disabled_background = theme.palette.darkest_background();

        drop(theme);

        cx.update_entity(&state, |this, _cx| {
            if this.hovered {
                this.hover_progress = this
                    .anim_start_time
                    .elapsed()
                    .div_duration_f32(this.anim_duration)
                    .clamp(0.0, 1.0)
            } else {
                this.hover_progress = 1.0
                    - this
                        .anim_start_time
                        .elapsed()
                        .div_duration_f32(this.anim_duration)
                        .clamp(0.0, 1.0)
            }

            if this.hover_progress > 0.0 || this.hover_progress < 1.0 {
                window.request_animation_frame();
            }
        });
        let current_view = window.current_view();

        base.id(("button_container", state.entity_id()))
            .when_else(
                !disabled,
                |this| {
                    this.text_color(text_color)
                        .bg(background.lerp(
                            &hover_background,
                            ease_in_out(state.read(cx).hover_progress),
                        ))
                        .active(|style| style.bg(active_background).text_color(background))
                },
                |this| this.text_color(disabled_text_color).bg(disabled_background),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .id(("button_inner", state.entity_id()))
                    .when(!disabled, |this| {
                        this.on_hover(move |&hovered, window, cx| {
                            cx.update_entity(&state, |this, _cx| {
                                this.hovered = hovered;
                                this.hover_progress = if hovered { 0.0 } else { 1.0 };
                                this.anim_start_time = Instant::now();
                            });
                            window.on_next_frame(move |_window, cx| cx.notify(current_view));
                        })
                    }),
            )
            .tap_mut(|this| this.style().refine(&style))
            .children(children)
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}
