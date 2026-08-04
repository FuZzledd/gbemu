use crate::{
    GlobalState, WindowMap, WindowType,
    components::{root::Root, titlebar::TitleBar},
    theme::ThemeRegistry,
};
use chumsky::Parser;
use gbemu_core::debugging::{
    BREAKPOINT_REPORTER, StyledText as CoreStyledText, StyledTextOutput, TextWeight,
    command_parser, styled,
};
use gbemu_core::{DEBUGGING_ENABLED, debugging};
use gpui::*;
use gpui_elements::editable_text::{self, actions::DEFAULT_INPUT_CONTEXT, *};
use itertools::Itertools;
use std::sync::atomic::Ordering;
use uzi::using;

pub struct Debugger {
    text_input_state: Entity<EditableTextState>,
    output: Vec<StyledTextOutput>,
    output_list_state: ListState,
    input_scroll_handle: ScrollHandle,
    receiver_task: Option<Task<()>>,
}

impl Debugger {
    pub fn open(
        window: &mut Window,
        cx: &mut App,
    ) -> std::result::Result<gpui::WindowHandle<Root>, gpui::private::anyhow::Error> {
        let bounds = Bounds::centered(None, size(px(500.), px(500.0)), cx);
        cx.open_window(
            WindowOptions {
                window_decorations: Some(WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: Some("Debugger".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            Self::create_root,
        )
    }
    pub fn create_root(window: &mut Window, cx: &mut App) -> Entity<Root> {
        Root::new(Self::new(window, cx), window, cx)
    }

    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let entity = cx.new(|cx| Self {
            text_input_state: cx.new(|cx| EditableTextState::new(StringStorage::default(), cx)),
            output: vec![],
            input_scroll_handle: Default::default(),
            output_list_state: ListState::new(0, ListAlignment::Bottom, px(20.0)),
            receiver_task: None,
        });

        let weak_entity = entity.downgrade();

        entity.as_mut(cx).receiver_task = Some(cx.spawn(async move |cx| {
            loop {
                if let Ok(hit_breakpoints) = BREAKPOINT_REPORTER.1.recv_async().await {
                    let text = hit_breakpoints
                        .iter()
                        .map(|(id, data)| {
                            debugging::text![
                                styled(format!("Hit breakpoint #{id}: "))
                                    .with_weight(TextWeight::Bold),
                                data.to_string()
                            ]
                        })
                        .collect_vec();
                    if let Some(entity) = weak_entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            let gameboy = &mut *cx.global::<GlobalState>().gameboy.lock();
                            let state = gameboy.cpu.dump_state(&mut gameboy.context);

                            this.output.extend(text);
                            this.output.extend([vec![state.into()]]);

                            this.output_list_state.reset(this.output.len());
                        });
                    }
                }
            }
        }));

        cx.observe_release(&entity, |this, cx| {
            DEBUGGING_ENABLED.store(false, Ordering::Relaxed);
            cx.global_mut::<WindowMap>().remove(&WindowType::Debugger);
        })
        .detach();

        DEBUGGING_ENABLED.store(true, Ordering::Relaxed);

        entity
    }
}

impl Render for Debugger {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("debugger", entity_id));

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let lighter_background = theme.palette.lighter_background();
        let darker_background = theme.palette.darker_background();
        let dark_foreground = theme.palette.dark_foreground();
        let foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        drop(theme);

        let global_state = cx.global::<GlobalState>();
        let gameboy = global_state.gameboy.clone();

        div()
            .text_sm()
            .overflow_hidden()
            .max_size_full()
            .flex()
            .flex_auto()
            .flex_col()
            .items_stretch()
            .child(
                TitleBar::new(element_id.clone())
                    .flex()
                    .items_stretch()
                    .content_center()
                    .child(
                        div()
                            .flex_auto()
                            .flex()
                            .justify_center()
                            .items_center()
                            .child("Debugger"),
                    ),
            )
            .child(
                div()
                    .flex_auto()
                    .bottom_0()
                    .mt_auto()
                    .p_2()
                    .overflow_hidden()
                    .child(
                        div()
                            .key_context(DEFAULT_INPUT_CONTEXT)
                            .rounded_sm()
                            .border_1()
                            .border_color(border)
                            .text_color(foreground)
                            .flex()
                            .flex_col()
                            .flex_auto()
                            .size_full()
                            .max_w_full()
                            .justify_start()
                            .child(
                                list(
                                    self.output_list_state.clone(),
                                    cx.processor(move |this, idx, window, cx| {
                                        div()
                                            .px_2()
                                            .text_color(foreground)
                                            .font_family("Fira Mono")
                                            .text_sm()
                                            .child(styled_output_to_text(&this.output[idx], cx))
                                            .into_any()
                                    }),
                                )
                                .flex_basis(relative(1.0))
                                .flex_grow_0()
                                .flex_shrink()
                                .bg(lighter_background)
                                .whitespace_nowrap(),
                            )
                            .child(
                                text_input((element_id.clone(), "input_line"))
                                    .flex_basis(auto())
                                    .flex_grow_0()
                                    .flex_shrink()
                                    .min_w_0()
                                    .w_full()
                                    .max_w_full()
                                    .border_t_1()
                                    .border_color(border)
                                    .p_1()
                                    .bg(darker_background)
                                    .overflow_scroll()
                                    .state(self.text_input_state.downgrade())
                                    .caret_blink_interval_500ms()
                                    .caret_blink_interval_500ms()
                                    .track_scroll(&self.input_scroll_handle)
                                    .whitespace_nowrap()
                                    .placeholder("Type \"help\" for a list of commands")
                                    .placeholder_color(dark_foreground.into())
                                    .font_family("Fira Mono"),
                            )
                            .capture_action::<editable_text::actions::Enter>(cx.listener(using!(
                                [self.text_input_state],
                                move |this, action, window, cx| {
                                    let input =
                                        text_input_state.update(cx, |text_input_state, cx| {
                                            let input = text_input_state.as_str().to_string();
                                            text_input_state.emplace("", cx);
                                            input
                                        });

                                    if input.is_empty() {
                                        return;
                                    }

                                    this.output.push(debugging::text![input.clone()]);

                                    match command_parser().parse(&input).into_result() {
                                        Ok(command) => {
                                            command.handle(&mut this.output, &mut gameboy.lock())
                                        }
                                        Err(errors) => this.output.push(vec![
                                            CoreStyledText::new(format!(
                                                "Invalid command: {}",
                                                errors
                                                    .into_iter()
                                                    .map(|x| x.to_string())
                                                    .join(", ")
                                            ))
                                            .with_color(|palette| palette.red()),
                                        ]),
                                    }

                                    this.output_list_state.reset(this.output.len());
                                }
                            ))),
                    ),
            )
    }
}

fn styled_output_to_text(styled_text_output: &StyledTextOutput, cx: &mut App) -> StyledText {
    let built = styled_text_output.iter().fold(
        (String::default(), vec![]),
        |(mut built_text, mut highlights), text| {
            match text {
                CoreStyledText::Default(text) => {
                    built_text.push_str(&text);
                }
                CoreStyledText::Styled(styled) => {
                    use gbemu_core::debugging::TextStyle;

                    let start_idx = built_text.len();
                    let end_idx = built_text.len() + styled.text.len();
                    let highlight = HighlightStyle {
                        color: styled.palette_fn.clone().map(|func| {
                            func(cx.global::<ThemeRegistry>().current_theme().palette.clone())
                                .into()
                        }),
                        font_weight: styled.weight.map(|weight| match weight {
                            TextWeight::Normal => FontWeight::NORMAL,
                            TextWeight::Bold => FontWeight::BOLD,
                        }),
                        font_style: styled.style.map(|style| match style {
                            TextStyle::Normal => FontStyle::Normal,
                            TextStyle::Italics => FontStyle::Oblique,
                        }),
                        ..Default::default()
                    };
                    highlights.push((start_idx..end_idx, highlight));
                    built_text.push_str(&styled.text);
                }
            }
            (built_text, highlights)
        },
    );

    StyledText::new(built.0).with_highlights(built.1)
}

impl Drop for Debugger {
    fn drop(&mut self) {}
}
