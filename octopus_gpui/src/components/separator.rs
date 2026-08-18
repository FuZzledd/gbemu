use gpui::*;
use tap::Tap;

use crate::theme::ThemeRegistry;

#[derive(IntoElement)]
pub struct Separator {
    style: StyleRefinement,
}

impl Styled for Separator {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Separator {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
        }
    }
}

impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for Separator {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();
        let border = theme.palette.gray();
        drop(theme);

        div()
            .m_2()
            .h_0()
            .w_full()
            .border_b_1()
            .border_color(border)
            .tap_mut(|this| this.style().refine(&self.style))
    }
}
