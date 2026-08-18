use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use gpui::*;
use palette::IntoColor;
use tap::Tap;
use uzi::using;

use crate::{GlobalState, RenderState};

#[derive(IntoElement, Debug)]
pub struct Screen {
    render_state: Option<Arc<RenderState>>,
    frame_delta: Arc<AtomicU32>,
    style: StyleRefinement,
}

impl Screen {
    pub fn new(render_state: Option<Arc<RenderState>>, frame_delta: Arc<AtomicU32>) -> Self {
        Self {
            render_state,
            frame_delta,
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Screen {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Screen {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            using!(
                [self.render_state, self.frame_delta],
                move |bound, _, window, cx| {
                    let bounds_size = bound.size;
                    let global_state = cx.global::<GlobalState>();

                    let scale_factor = if global_state.fixed_size {
                        global_state.scale_factor as f32
                    } else if global_state.integer_scaling {
                        (bounds_size.height / px(144.0))
                            .min(bounds_size.width / px(160.0))
                            .max(1.0)
                            .floor()
                    } else {
                        (bounds_size.height / px(144.0))
                            .min(bounds_size.width / px(160.0))
                            .max(1.0)
                    };

                    let screen_size: gpui::Size<Pixels> =
                        size(px(160.0 * scale_factor), px(144.0 * scale_factor));

                    let origin =
                        bound.center() - point(screen_size.width / 2.0, screen_size.height / 2.0);

                    let origin = window.pixel_snap_point(origin);

                    // window.paint_quad(fill(bounds(origin, screen_size), rgb(0x000000)));

                    window.paint_surface(
                        bounds(origin, screen_size),
                        Arc::clone(&render_state.unwrap().screen_texture) as _,
                        size(160.into(), 144.into()),
                        Some(if global_state.linear_filtering {
                            SamplerType::Linear
                        } else {
                            SamplerType::Nearest
                        }),
                    );

                    if global_state.show_fps {
                        let text = format!(
                            "{:.1}",
                            1.0 / f32::from_bits(frame_delta.load(Ordering::Relaxed))
                        );

                        let mut font = font("Fira Mono");
                        font.fallbacks = Some(FontFallbacks::from_fonts(vec![
                            "Consolas".into(),
                            "Courier".into(),
                            "Courier New".into(),
                            "Noto Mono".into(),
                        ]));

                        let shaped = window.text_system().shape_line(
                            text.clone().into(),
                            px(12.0),
                            &[TextRun {
                                len: text.len(),
                                font,
                                color: rgb(0xCCCCCC).into_color(),
                                background_color: Some(black()),
                                underline: None,
                                strikethrough: None,
                                letter_spacing: None,
                            }],
                            None,
                        );

                        shaped
                            .paint_background(
                                origin,
                                px(12.0),
                                TextAlign::Right,
                                Some(screen_size.width),
                                window,
                                cx,
                            )
                            .unwrap();
                        shaped
                            .paint(
                                origin,
                                px(12.0),
                                TextAlign::Right,
                                Some(screen_size.width),
                                window,
                                cx,
                            )
                            .unwrap();
                    }

                    window.request_animation_frame();
                }
            ),
        )
        .tap_mut(|this| this.style().refine(&self.style))
    }
}
