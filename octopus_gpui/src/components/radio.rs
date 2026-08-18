use core::time::Duration;

use crate::{components::Button, theme::ThemeRegistry};
use gpui::{prelude::FluentBuilder, *};
use tap::Tap;

#[derive(IntoElement)]
pub struct RadioButton {
    pub checked: bool,
    pub id: ElementId,
    pub on_checked: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    pub style: StyleRefinement,
}

impl RadioButton {
    pub fn new(checked: bool, id: impl Into<ElementId>) -> Self {
        RadioButton {
            checked,
            id: id.into(),
            on_checked: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn on_checked(mut self, callback: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_checked = Some(Box::new(callback));
        self
    }
}

impl Styled for RadioButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for RadioButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();
        let background = theme.palette.background();
        let foreground = theme.palette.foreground();
        let border = theme.palette.gray();
        drop(theme);

        Button::new(
            Duration::from_millis(100),
            (self.id.clone(), "radio_button"),
        )
        .role(Role::RadioButton)
        .border_color(border)
        .border_1()
        .rounded_full()
        .w_auto()
        .h_4_5()
        .aspect_square()
        .p_0p5()
        .child(
            div()
                .when(self.checked, |this| this.bg(foreground))
                .size_full()
                .rounded_full()
                .aspect_square(),
        )
        .when_some(self.on_checked, move |this, on_checked| {
            this.on_click(move |event, window, cx| on_checked(&self.checked, window, cx))
        })
        .tap_mut(|this| this.style().refine(&self.style))
    }
}
