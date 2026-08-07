use crate::components::button::Button;
use crate::components::serializedaction::SerializedAction;
use crate::settings::KeybindProvider;
use crate::theme::ThemeRegistry;
use gpui::prelude::*;
use gpui::*;
use std::time::Duration;
use tap::Tap;
use uzi::using;

pub struct BindableButton<S: KeybindProvider + 'static> {
    id: ElementId,
    serialized_action: SerializedAction,
    settings: Entity<S>,
    is_binding: bool,
    binding_timer: Option<Task<()>>,
    focus_handle: FocusHandle,
    _settings_subscription: Subscription,
    button: Entity<Button>,
}

impl<S: KeybindProvider + 'static> BindableButton<S> {
    pub fn new(
        id: impl Into<ElementId>,
        serialized_action: SerializedAction,
        settings: Entity<S>,
        cx: &mut Context<Self>,
    ) -> Self {
        eprintln!("Created BindableButton");
        let id = id.into();
        let _settings_subscription = cx.observe(&settings, |this, _settings, cx| {
            this.update_button_text(cx);
            cx.notify();
        });

        // Use the passed-in ID for the button
        let button = Button::new(cx, (id.clone(), "inner"), Duration::from_millis(50));

        let mut this = Self {
            id,
            serialized_action,
            settings,
            is_binding: false,
            binding_timer: None,
            focus_handle: cx.focus_handle(),
            _settings_subscription,
            button,
        };

        this.update_button_text(cx);
        this
    }

    fn update_button_text(&mut self, cx: &mut Context<Self>) {
        let button_text = Self::get_button_text(&self.settings.read(cx), &self.serialized_action);
        self.button.update(cx, |button, cx| {
            button.clear_children();
            button.add_child(move |_window, _cx| div().child(button_text.clone()));
            cx.notify();
        });
    }

    /// Helper to format keybind strings cleanly
    fn get_button_text(settings: &S, serialized_action: &SerializedAction) -> String {
        settings
            .get_bindings_for_action(serialized_action)
            .unwrap_or_else(|| "None".into())
    }
}

impl<S: KeybindProvider + 'static> Render for BindableButton<S> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();
        let border = theme.palette.gray();
        drop(theme);

        div()
            .id(self.id.clone())
            .child(self.button.clone())
            .rounded_sm()
            .border_color(border)
            .border_1()
            .flex_1()
            .tap_mut(|this| {
                this.style().flex_grow = Some(0.5);
                this.style().flex_shrink = Some(0.5);
            })
            .w_full()
            .m_0p5()
            .justify_center()
            .items_baseline()
            .on_click(cx.listener(using!(
                [self.focus_handle],
                move |this, event: &ClickEvent, window, cx| {
                    if event.standard_click() {
                        window.focus(&focus_handle, cx);
                        this.is_binding = true;

                        this.binding_timer = Some(cx.spawn(
                            |weak_self: WeakEntity<BindableButton<S>>, cx: &mut AsyncApp| {
                                let mut app = cx.clone();
                                async move {
                                    app.background_executor()
                                        .timer(Duration::from_secs(3))
                                        .await;
                                    let _ = weak_self.update(&mut app, |this, cx| {
                                        this.is_binding = false;
                                        cx.notify();
                                    });
                                }
                            },
                        ));
                    }
                }
            )))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.is_binding {
                    this.settings.update(cx, |settings, cx| {
                        settings.set_binding_for_action(
                            &this.serialized_action,
                            event.keystroke.unparse(),
                        );
                        cx.notify();
                    });
                    this.binding_timer = None;
                    this.is_binding = false;
                    cx.notify();
                }
            }))
            .focusable()
            .track_focus(&self.focus_handle)
    }
}
