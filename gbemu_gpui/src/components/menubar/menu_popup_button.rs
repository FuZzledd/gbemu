use crate::{components::button::Button, theme::ThemeRegistry};
use core::time::Duration;
use gpui::{prelude::*, *};

#[derive(IntoElement)]
pub struct PopupButton {
    id: ElementId,
    button: Entity<Button>,
    disabled: bool,
}

impl PopupButton {
    pub fn new(cx: &mut App, id: impl Into<ElementId>) -> Self {
        let id = id.into();
        let button = Button::new(cx, id.clone(), Duration::from_millis(50));

        Self {
            id,
            button,
            disabled: false,
        }
    }

    pub fn with_button(button: Entity<Button>, id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            button,
            disabled: false,
        }
    }

    pub fn disabled(mut self, cx: &mut App, disabled: bool) -> Self {
        self.disabled = disabled;
        self.button.update(cx, |btn, _| {
            btn.set_disabled(disabled);
        });
        self
    }

    pub fn on_click(
        self,
        cx: &mut App,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.button.update(cx, |btn, _| {
            btn.on_click(listener);
        });
        self
    }

    pub fn on_hover(
        self,
        cx: &mut App,
        listener: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.button.update(cx, |btn, _| {
            btn.on_hover(listener);
        });
        self
    }
}

impl ParentElement for PopupButton {
    fn extend(&mut self, _children: impl IntoIterator<Item = AnyElement>) {
        // Handled via child methods or directly on the button entity during construction
    }
}

impl RenderOnce for PopupButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();
        let background = theme.palette.darker_background();
        let hover_background = theme.palette.background();
        let foreground = theme.palette.foreground();
        let darker_foreground = theme.palette.dark_foreground();
        drop(theme);

        let disabled = self.disabled;

        // Configure button colors without mutating state on every frame
        self.button.update(cx, |btn, _| {
            btn.set_background(background)
                .set_hover_background(hover_background);
        });

        div()
            .id(self.id.clone())
            .on_hover(move |hovered, _window, _cx| {
                if *hovered {
                    eprintln!("PopupButton {:?} hover event: {}", self.id, hovered);
                }
            })
            .w_full()
            .text_color(if disabled {
                darker_foreground
            } else {
                foreground
            })
            .child(self.button)
    }
}
