use crate::{components::DefaultableInto, theme::ThemeRegistry};
use core::time::Duration;
use gpui::AsyncWindowContext;
use gpui::{Lerp, prelude::*, *};
use std::time::Instant;
use tap::Tap;
pub type ElementBuilder = Box<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>;

/// Standard cubic ease-in-out curve
fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub struct Button {
    id: ElementId,
    anim_duration: Duration,
    background: Option<Rgba>,
    hover_background: Option<Rgba>,
    style: StyleRefinement,
    children: Vec<ElementBuilder>,
    disabled: bool,
    pub on_hover: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    pub on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,

    hovered: bool,
    progress: f32,
    anim_task: Option<Task<()>>,
}

impl Button {
    pub fn new(
        cx: &mut App,
        id: impl Into<ElementId>,
        hover_anim_duration: Duration,
    ) -> Entity<Self> {
        let id = id.into();
        cx.new(|_| Self {
            id,
            anim_duration: hover_anim_duration,
            background: None,
            hover_background: None,
            style: StyleRefinement::default(),
            children: vec![],
            disabled: false,
            on_hover: None,
            on_click: None,
            hovered: false,
            progress: 0.0,
            anim_task: None,
        })
    }

    pub fn set_background<T>(&mut self, color: impl DefaultableInto<Rgba, T>) -> &mut Self {
        self.background = color.defaultable_into();
        self
    }

    pub fn set_hover_background<T>(&mut self, color: impl DefaultableInto<Rgba, T>) -> &mut Self {
        self.hover_background = color.defaultable_into();
        self
    }

    pub fn set_disabled(&mut self, disabled: bool) -> &mut Self {
        self.disabled = disabled;
        self
    }

    pub fn on_hover(
        &mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> &mut Self {
        self.on_hover = Some(Box::new(handler));
        self
    }

    pub fn on_click(
        &mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> &mut Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub fn clear_children(&mut self) -> &mut Self {
        self.children.clear();
        self
    }

    pub fn add_child<E: IntoElement + 'static>(
        &mut self,
        builder: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> &mut Self {
        self.children
            .push(Box::new(move |win, cx| builder(win, cx).into_any_element()));
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for Button {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let eased = ease_in_out(self.progress);

        let theme = cx.global::<ThemeRegistry>().current_theme();
        let resolved_background = self
            .background
            .unwrap_or_else(|| theme.palette.darker_background().into());
        let resolved_hover_background = self
            .hover_background
            .unwrap_or_else(|| theme.palette.gray().into());
        let current_color = resolved_background.lerp(&resolved_hover_background, eased);

        let active_background: Rgba = theme.palette.white().into();
        let text_color = theme.palette.foreground();
        let disabled_text_color = theme.palette.dark_foreground();
        let disabled_background = theme.palette.darkest_background();
        drop(theme);

        let rendered_children: Vec<AnyElement> = self
            .children
            .iter()
            .map(|builder| builder(window, &mut *cx))
            .collect();

        div()
            .id(self.id.clone())
            .w_full()
            .flex()
            .tap_mut(|div| div.style().refine(&self.style))
            .when_else(
                !self.disabled,
                |this| {
                    let mut this = this
                        .cursor_default()
                        .text_color(text_color)
                        .bg(current_color)
                        .active(move |style| {
                            style.bg(active_background).text_color(resolved_background)
                        })
                        .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                            let is_hovered = *hovered;

                            if this.hovered != is_hovered {
                                this.hovered = is_hovered;

                                // Guard against 0ms animation duration causing NaN
                                let duration = this.anim_duration.as_secs_f32().max(0.001);
                                let start_progress = this.progress;
                                let start_time = Instant::now();

                                // Assigning `anim_task` cancels any currently running task automatically
                                // Assigning `anim_task` automatically drops and cancels any previous task
                                this.anim_task = Some(cx.spawn(
                                    async move |weak_self: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {

                                        loop {
                                            let executor = cx.background_executor();

                                            // Await the timer using the executor handle (doesn't hold `cx` across .await)
                                            executor.timer(Duration::from_millis(16)).await;

                                            let done = weak_self
                                                .update(cx, |this, cx| {
                                                    let elapsed =
                                                        start_time.elapsed().as_secs_f32();

                                                    if is_hovered {
                                                        this.progress = (start_progress
                                                            + elapsed / duration)
                                                            .min(1.0);
                                                    } else {
                                                        this.progress = (start_progress
                                                            - elapsed / duration)
                                                            .max(0.0);
                                                    }

                                                    cx.notify();

                                                    if is_hovered {
                                                        this.progress >= 1.0
                                                    } else {
                                                        this.progress <= 0.0
                                                    }
                                                })
                                                .unwrap_or(true);

                                            if done {
                                                break;
                                            }
                                        }
                                    },
                                ));
                            }

                            if let Some(ref on_hover) = this.on_hover {
                                on_hover(hovered, window, &mut *cx);
                            }
                        }));

                    if self.on_click.is_some() {
                        this =
                            this.on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                if let Some(ref on_click) = this.on_click {
                                    on_click(event, window, &mut *cx);
                                }
                            }));
                    }
                    this
                },
                |this| this.text_color(disabled_text_color).bg(disabled_background),
            )
            .children(rendered_children)
    }
}
