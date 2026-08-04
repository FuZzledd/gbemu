use gpui::{prelude::*, *};
use uzi::using;

use crate::{components::menubar::ElementExt, theme::ThemeRegistry};

#[derive(IntoElement)]
pub struct Scrollbar {
    pub id: ElementId,
    pub offset: Pixels,
    pub max_offset: Pixels,
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

        let theme = cx.global::<ThemeRegistry>().current_theme();
        let border = theme.palette.gray();

        drop(theme);

        div()
            .size_full()
            .absolute()
            .top_0()
            .left_0()
            .child(
                div()
                    .absolute()
                    .right_2()
                    .top(scrollbar_offset)
                    .h(px(scrollbar_height))
                    .bg(border)
                    .w_2()
                    .rounded_xl(),
            )
            .on_bounds_prepaint(using!([viewport_bounds], move |bounds, _window, cx| {
                viewport_bounds.write(cx, bounds);
            }))
    }
}
