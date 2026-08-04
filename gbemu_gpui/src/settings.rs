use crate::components::{button::Button, root::CloseRequestEvent, scrollbar::Scrollbar};
use crate::theme::ThemeRegistry;
use crate::{APP, WindowMap, WindowType};
use crate::{EtceteraStrategy, components::titlebar::TitleBar};
use crate::{components::root::Root, reload_keys};
use better_default::Default;
use convert_case::Casing;
use etcetera::AppStrategy;
use gbemu_common::theme::Color;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_elements::editable_text::{EditableTextState, StringStorage, text_input};
use itertools::Itertools;
use palette::Srgba;
use serde::de::{SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use std::time::Duration;
use std::{
    borrow::Cow,
    ops::{Deref, DerefMut},
};
use std::{collections::HashMap, fs};
use std::{fmt::Debug, path::PathBuf};
use tap::{Conv, Tap};
use uzi::using;

#[macro_export]
macro_rules! binds {
    ($(($display:expr, $action:expr)),* $(,)?) => {
        vec![$(( $display, $action.to_serialized() )),*]
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Settings {
    #[serde(default)]
    pub(crate) input: InputSettings,
    #[serde(default)]
    pub(crate) emulator: EmulatorSettings,
}
impl Global for Settings {}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct InputSettings {
    #[serde(default)]
    pub(crate) keybinds: Keymap,
}

impl InputSettings {
    pub fn set_keybinds(&self, cx: &mut App) {
        let keyboard_mapper = cx.keyboard_mapper().clone();
        cx.bind_keys(self.keybinds.values().flat_map(|keybinds| {
            keybinds.iter().map(|(keystroke, action)| {
                KeyBinding::load(
                    keystroke,
                    action.boxed_clone(),
                    None,
                    false,
                    None,
                    keyboard_mapper.as_ref(),
                )
                .unwrap()
            })
        }));
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct EmulatorSettings {
    #[serde(default)]
    #[default("Catppuccin Frappe".into())]
    pub(crate) theme: Cow<'static, str>,
    #[serde(default)]
    pub(crate) library_path: PathBuf,
}

pub trait SerializableAction: Action {
    fn to_serialized(&self) -> SerializedAction;
}

impl<T> SerializableAction for T
where
    T: Action + Serialize,
{
    fn to_serialized(&self) -> SerializedAction {
        let value = serde_json::to_value(self).unwrap();
        match value {
            Value::Null => SerializedAction(self.name().into(), self.boxed_clone()),
            other => SerializedAction([self.name().into(), other].into(), self.boxed_clone()),
        }
    }
}

pub struct SerializedAction(pub Value, pub Box<dyn Action>);

impl Clone for SerializedAction {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.boxed_clone())
    }
}

impl SerializedAction {
    pub fn json_value(&self) -> Value {
        self.0.clone()
    }

    pub fn action(&self) -> Box<dyn Action> {
        self.1.boxed_clone()
    }
}

#[derive(Default, Debug)]
pub struct Keymap {
    pub bindings: HashMap<Value, Vec<(String, Box<dyn Action>)>>,
}

impl Eq for Keymap {}

impl PartialEq for Keymap {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        self.iter().all(|(k, v)| {
            other
                .get(k)
                .map(|binds| binds.iter().map(|binds| binds.0.clone()).collect_vec())
                == Some(v.iter().map(|binds| binds.0.clone()).collect_vec())
        })
    }
}

impl Clone for Keymap {
    fn clone(&self) -> Self {
        Keymap {
            bindings: self
                .bindings
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        value
                            .iter()
                            .map(|(keystroke, action)| (keystroke.clone(), action.boxed_clone()))
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

impl Deref for Keymap {
    type Target = HashMap<Value, Vec<(String, Box<dyn Action>)>>;

    fn deref(&self) -> &Self::Target {
        &self.bindings
    }
}
impl DerefMut for Keymap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bindings
    }
}

impl<'de> Deserialize<'de> for Keymap {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BindingsVisitor;
        impl<'de> Visitor<'de> for BindingsVisitor {
            type Value = Vec<(String, Value)>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a map")
            }

            fn visit_seq<A>(self, mut v: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut bindings = vec![];

                while let Some(binding) = v.next_element::<(String, serde_json::Value)>()? {
                    bindings.push(binding);
                }

                Ok(bindings)
            }
        }

        let raw_bindings = deserializer.deserialize_seq(BindingsVisitor)?;

        let mut bindings = HashMap::<Value, Vec<(String, Box<dyn Action>)>>::new();

        APP.with(move |cx| {
            let taken_cx = cx.take();
            let res = {
                let cx = taken_cx
                    .as_ref()
                    .ok_or(de::Error::custom("Couldn't get app context"))?
                    .clone();

                for (keystrokes, action_raw) in raw_bindings {
                    let mut action_input: Option<SharedString> = None;
                    let action = match action_raw.clone() {
                        Value::String(ref name) => cx.update(move |cx| {
                            cx.build_action(name, None)
                                .map_err(|err| de::Error::custom(format!("Couldn't build action {err}, name={name}")))
                        })?,
                        Value::Array(array) => {
                            if array.len() != 2 {
                                return Err(de::Error::custom(
                                    "Expected a two-element of array of [name, data]",
                                ));
                            }
                            let serde_json::Value::String(ref name) = array[0] else {
                                return Err(de::Error::custom(
                                    "Expected a string as the first element of the array",
                                ));
                            };

                            cx.update(|cx| {
                                action_input = Some(array[1].to_string().into());
                                cx.build_action(name, Some(array[1].clone()))
                                    .map_err(|err| de::Error::custom(format!("Couldn't build action {err}, name={name}, action_input = {action_input:?}")))
                            })?
                        }
                        _ => return Err(de::Error::custom("Expected a valid action")),
                    };

                    bindings.entry(action_raw).or_default().push((keystrokes, action));
                }

                Ok(Keymap { bindings })
            };
            cx.set(taken_cx);
            res
        })
    }
}

impl Serialize for Keymap {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_seq(Some(self.bindings.len()))?;

        for (action_raw, action_bindings) in self.bindings.iter() {
            for (keystrokes, _) in action_bindings {
                s.serialize_element(&(keystrokes, action_raw.clone()))?;
            }
        }
        s.end()
    }
}

pub struct SettingsWindow {
    pub settings: Entity<Settings>,
    pub input: AnyView,
    emulator: AnyView,
}

impl SettingsWindow {
    pub fn open(
        _window: &mut Window,
        cx: &mut App,
    ) -> std::result::Result<gpui::WindowHandle<Root>, gpui::private::anyhow::Error> {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_decorations: Some(WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: Some("Settings".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            Self::create_root,
        )
    }
    pub fn create_root(window: &mut Window, cx: &mut App) -> Entity<Root> {
        let settings_window = Self::new(window, cx);
        let root = Root::new(settings_window.clone(), window, cx);
        root.update(cx, |root, _cx| {
            root.on_close_request(move |window, cx| {
                settings_window.update(cx, |this, cx| {
                    if this.settings.read(cx) != cx.global::<Settings>() {
                        let answer = window.prompt(
                            PromptLevel::Warning,
                            "There are unsaved changes, are you sure you want to close the window?",
                            None,
                            &[PromptButton::cancel("Cancel"), PromptButton::ok("Yes")],
                            cx,
                        );

                        cx.spawn(async move |weak_this, cx| {
                            let answer = answer.await.unwrap_or(0);
                            if answer == 1 {
                                cx.with_window(weak_this.entity_id(), |window, _cx| {
                                    window.remove_window();
                                })
                                .unwrap();
                            }
                        })
                        .detach();
                    } else {
                        window.remove_window()
                    }
                })
            })
        });
        root
    }

    pub fn new(_window: &mut Window, cx: &mut App) -> Entity<Self> {
        let settings = cx.new(|cx| cx.global::<Settings>().clone());

        let input = AnyView::from(InputSettingsTab::new(settings.clone(), cx));
        let emulator = AnyView::from(EmulatorSettingsTab::new(settings.clone(), cx));

        let entity = cx.new(|_cx| Self {
            settings,
            input,
            emulator,
        });

        cx.observe_release(&entity, |_this, cx| {
            cx.global_mut::<WindowMap>().remove(&WindowType::Settings);
        })
        .detach();

        entity
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use palette::Darken;

        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("Settings", entity_id));

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let lighter_background = theme.palette.lighter_background();
        let darker_background = theme.palette.darker_background();
        let _darkest_background = theme.palette.darkest_background();
        let background = theme.palette.background();
        let _dark_foreground = theme.palette.dark_foreground();
        let _foreground = theme.palette.foreground();
        let lighter_blue = theme.palette.blue();
        let blue = Srgba::<f32>::from_format(Srgba::from(lighter_blue))
            .darken(0.2)
            .conv::<Color>();
        let border = theme.palette.gray();

        drop(theme);

        let unsaved_changes =
            window.use_keyed_state((element_id.clone(), "unsaved_changes"), cx, |_, _| false);

        cx.observe(
            &self.settings,
            using!([unsaved_changes], move |_this, settings, cx| {
                unsaved_changes.write(cx, settings.read(cx) != cx.global::<Settings>());
            }),
        )
        .detach();

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_stretch()
            .child(
                TitleBar::new((element_id.clone(), "Titlebar"))
                    .flex()
                    .items_stretch()
                    .content_center()
                    .child(
                        div()
                            .flex_auto()
                            .flex()
                            .justify_center()
                            .items_center()
                            .child("Settings"),
                    ),
            )
            .child(div().flex_auto().items_stretch().child(TabBar::new(
                (element_id.clone(), "Tab bar"),
                [
                    Tab::new("Game", div().size_full().bg(background).child("Game stuff")),
                    Tab::new(
                        "Video",
                        div().size_full().bg(background).child("Video stuff"),
                    ),
                    Tab::new("Keyboard", self.input.clone()),
                    Tab::new("Emulator", self.emulator.clone()),
                ],
            )))
            .child(
                div()
                    .bg(darker_background)
                    .border_color(border)
                    .border_t_1()
                    .flex()
                    .justify_end()
                    .items_baseline()
                    .p_1()
                    .gap_2()
                    .child(
                        Button::new(Duration::from_millis(100), (element_id.clone(), "Close"))
                            .background(lighter_background)
                            .px_2()
                            .py(rems(0.1))
                            .rounded_sm()
                            .child("Close")
                            .on_click(cx.listener(move |_this, event: &ClickEvent, window, cx| {
                                if event.standard_click() {
                                    window.root::<Root>().unwrap().unwrap().update(cx, |_, cx| {
                                        cx.emit(CloseRequestEvent);
                                    })
                                }
                            })),
                    )
                    .child(
                        Button::new(Duration::from_millis(100), (element_id.clone(), "Save"))
                            .background(blue)
                            .hover_background(lighter_blue)
                            .px_2()
                            .py(rems(0.1))
                            .child("Save")
                            .rounded_sm()
                            .when_else(
                                *unsaved_changes.read(cx),
                                using!([unsaved_changes], |this| {
                                    this.on_click(cx.listener(
                                        move |this, event: &ClickEvent, _window, cx| {
                                            if event.standard_click() {
                                                let settings = this.settings.read(cx).clone();
                                                let settings_path = cx
                                                    .global::<EtceteraStrategy>()
                                                    .in_config_dir("config.json");
                                                fs::write(
                                                    settings_path,
                                                    serde_json::to_string_pretty(&settings)
                                                        .unwrap(),
                                                )
                                                .unwrap();
                                                cx.set_global(settings);
                                                reload_keys(cx);
                                                unsaved_changes.write(cx, false);
                                            }
                                        },
                                    ))
                                }),
                                |this| this.disabled(),
                            ),
                    ),
            )
    }
}

pub struct EmulatorSettingsTab {
    settings: Entity<Settings>,
    library_path_state: Entity<EditableTextState>,
}

impl EmulatorSettingsTab {
    pub fn new(settings: Entity<Settings>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            library_path_state: cx.new(|cx| {
                EditableTextState::new(
                    StringStorage::from(settings.read(cx).emulator.library_path.to_string_lossy()),
                    cx,
                )
            }),
            settings,
        })
    }
}

impl Render for EmulatorSettingsTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("emulator_settings", entity_id));

        let theme = cx.global::<ThemeRegistry>().current_theme();
        let foreground = theme.palette.foreground();
        let border = theme.palette.gray();
        let _background = theme.palette.background();
        let darker_background = theme.palette.darker_background();

        drop(theme);

        div()
            .text_color(foreground)
            .size_full()
            .p_8()
            .flex()
            .flex_col()
            .items_stretch()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child("ROM Library Path")
                    .child(
                        text_input((element_id.clone(), "rom library text input"))
                            .ml_2()
                            .flex()
                            .flex_1()
                            .state(self.library_path_state.downgrade())
                            .px_1()
                            .py(rems(0.15))
                            .rounded_l_md()
                            .border_1()
                            .border_color(border)
                            .items_baseline()
                            .bg(darker_background),
                    )
                    .child(
                        Button::new(
                            Duration::from_millis(200),
                            (element_id.clone(), "rom library browse"),
                        )
                        .px_2()
                        .py(rems(0.1))
                        .rounded_r_md()
                        .border_1()
                        .border_l_0()
                        .border_color(border)
                        .child("Browse"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child("ROM Library Path")
                    .child(
                        text_input((element_id.clone(), "rom library text input"))
                            .ml_2()
                            .flex()
                            .flex_1()
                            .state(self.library_path_state.downgrade())
                            .px_1()
                            .py(rems(0.15))
                            .rounded_l_md()
                            .border_1()
                            .border_color(border)
                            .items_baseline()
                            .bg(darker_background),
                    )
                    .child(
                        Button::new(
                            Duration::from_millis(200),
                            (element_id.clone(), "rom library browse"),
                        )
                        .px_2()
                        .py(rems(0.1))
                        .rounded_r_md()
                        .border_1()
                        .border_l_0()
                        .border_color(border)
                        .child("Browse"),
                    ),
            )
    }
}

pub struct InputSettingsTab {
    settings: Entity<Settings>,
    bind_sections: [Entity<InputSection>; 5],
    list_state: ListState,
}

impl InputSettingsTab {
    fn new(settings: Entity<Settings>, cx: &mut App) -> Entity<Self> {
        use crate::actions::*;

        let bind_sections = [
            (
                "Game",
                binds![
                    ("A", game::A),
                    ("B", game::B),
                    ("Up", game::Up),
                    ("Down", game::Down),
                    ("Left", game::Left),
                    ("Right", game::Right),
                    ("Start", game::Start),
                    ("Select", game::Select),
                ],
            ),
            (
                "Playback",
                binds![
                    ("Toggle Pause", playback::TogglePause),
                    ("Fast-forward (Hold)", playback::FastForward),
                    ("Fast-forward (Toggle)", playback::ToggleFastForward),
                    ("Step Tick", playback::StepTick),
                    ("Step Frame", playback::StepFrame)
                ],
            ),
            (
                "Video",
                binds![
                    ("Toggle Fullscreen", video::ToggleFullscreen),
                    ("Toggle Integer Scaling", video::ToggleIntegerScaling),
                    ("Toggle Show FPS", video::ToggleShowFps),
                    ("Toggle Resize to Fit Window", video::ToggleFixedSize),
                    ("Toggle Filtering", video::ToggleLinearFiltering)
                ],
            ),
            (
                "Tools",
                binds![
                    ("Open Settings", tools::Settings),
                    ("Open Debugger", tools::ToggleDebugger),
                ],
            ),
            (
                "File",
                binds![("Open ROM", file::OpenRom), ("Exit", file::Exit),],
            ),
        ];

        cx.new(|cx| Self {
            list_state: ListState::new(bind_sections.len(), ListAlignment::Top, px(25.0))
                .measure_all(),
            bind_sections: bind_sections
                .map(|(name, binds)| InputSection::new(name, binds, settings.clone(), cx)),
            settings,
        })
    }
}

impl Render for InputSettingsTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _keybinds = &self.settings.read(cx).input.keybinds;

        let theme = cx.global::<ThemeRegistry>().current_theme();
        let foreground = theme.palette.foreground();
        let _border = theme.palette.gray();
        let _background = theme.palette.background();
        drop(theme);

        div()
            .size_full()
            .child(
                list(
                    self.list_state.clone(),
                    using!([self.bind_sections], move |idx, _window, _cx| {
                        div()
                            .w_full()
                            .text_color(foreground)
                            .flex()
                            .flex_col()
                            .flex_nowrap()
                            .gap_2()
                            .items_stretch()
                            .child(bind_sections[idx].clone())
                            .into_any_element()
                    }),
                )
                .p_2()
                .text_color(foreground)
                .flex()
                .flex_col()
                .flex_nowrap()
                .gap_2()
                .items_stretch()
                .size_full(),
            )
            .child(Scrollbar {
                id: ElementId::from(("scrollbar", cx.entity_id())),
                offset: self.list_state.scroll_px_offset_for_scrollbar().y,
                max_offset: self.list_state.max_offset_for_scrollbar().y,
            })
    }
}

pub struct InputSection {
    section_name: &'static str,
    binds: Vec<(&'static str, SerializedAction)>,
    settings: Entity<Settings>,
}
impl InputSection {
    fn new<T: AppContext>(
        section_name: &'static str,
        binds: Vec<(&'static str, SerializedAction)>,
        settings: Entity<Settings>,
        cx: &mut T,
    ) -> Entity<Self> {
        cx.new(|_| Self {
            section_name,
            settings,
            binds,
        })
    }
}

impl Render for InputSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();
        let _foreground = theme.palette.foreground();
        let border = theme.palette.gray();
        let _background = theme.palette.background();
        let _lighter_background = theme.palette.lighter_background();

        drop(theme);

        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("input_section", entity_id));

        let (chunked, leftover) = self.binds.as_chunks::<2>();

        let settings = self.settings.clone();

        div()
            .flex()
            .items_stretch()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_center()
                    .items_baseline()
                    .mx_8()
                    .child(self.section_name)
                    .border_color(border)
                    .border_b_1(),
            )
            .child(
                div()
                    .mx_16()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .items_stretch()
                    .children(chunked.iter().map(|chunk| {
                        div()
                            .flex()
                            .justify_between()
                            .items_baseline()
                            .gap_2()
                            .children(chunk.iter().map(|bind| {
                                render_bind(
                                    bind.clone(),
                                    settings.clone(),
                                    element_id.clone(),
                                    window,
                                    cx,
                                )
                            }))
                    }))
                    .when(!leftover.is_empty(), |this| {
                        let bind = leftover[0].clone();
                        this.child(
                            div()
                                .flex()
                                .justify_between()
                                .items_baseline()
                                .gap_2()
                                .child(render_bind(
                                    bind,
                                    settings.clone(),
                                    element_id.clone(),
                                    window,
                                    cx,
                                ))
                                .child(div().flex().flex_1()),
                        )
                    }),
            )
    }
}

fn render_bind(
    bind: (&'static str, SerializedAction),
    settings: Entity<Settings>,
    element_id: ElementId,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let theme = cx.global::<ThemeRegistry>().current_theme();
    let _foreground = theme.palette.foreground();
    let _border = theme.palette.gray();
    let _background = theme.palette.background();
    let lighter_background = theme.palette.lighter_background();

    drop(theme);

    let name = bind.0;
    let bindable = window.use_keyed_state((element_id, name), cx, |_window, cx| {
        BindableButton::new(bind.1.clone(), settings, cx)
    });

    div()
        .bg(lighter_background)
        .flex()
        .gap_1()
        .flex_1()
        .items_stretch()
        .child(
            div()
                .flex()
                .justify_center()
                .items_baseline()
                .child(bind.0)
                .flex_1(),
        )
        .child(bindable)
}

pub struct BindableButton {
    serialized_action: SerializedAction,
    settings: Entity<Settings>,
    is_binding: bool,
    binding_timer: Option<Task<()>>,
    focus_handle: FocusHandle,
}

impl BindableButton {
    pub fn new(
        serialized_action: SerializedAction,
        settings: Entity<Settings>,
        cx: &mut App,
    ) -> Self {
        Self {
            serialized_action,
            settings,
            is_binding: false,
            binding_timer: None,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for BindableButton {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("bindable", entity_id));

        let theme = cx.global::<ThemeRegistry>().current_theme();
        let _foreground = theme.palette.foreground();
        let border = theme.palette.gray();
        let _background = theme.palette.background();
        let _lighter_background = theme.palette.lighter_background();

        drop(theme);

        let button_text =
            window.use_keyed_state((element_id.clone(), "button_text"), cx, |_window, cx| {
                self.settings
                    .read(cx)
                    .input
                    .keybinds
                    .bindings
                    .get(&self.serialized_action.json_value())
                    .map(|keybinds| {
                        keybinds
                            .iter()
                            .map(|(keystrokes, _)| {
                                keystrokes
                                    .split_whitespace()
                                    .map(|keystroke| {
                                        Keystroke::parse(keystroke)
                                            .unwrap()
                                            .to_string()
                                            .to_case(convert_case::Case::Train)
                                    })
                                    .join(" ")
                            })
                            .join(" | ")
                    })
                    .unwrap_or("None".into())
            });

        Button::new(Duration::from_millis(100), element_id)
            .child(
                div()
                    .when_else(
                        self.is_binding,
                        |this| this.child("..."),
                        |this| this.child(button_text.read(cx).clone()),
                    )
                    .flex()
                    .size_full()
                    .items_baseline()
                    .justify_center(),
            )
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
                        this.binding_timer = Some(cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(Duration::from_secs(3)).await;
                            this.update(cx, |this, _cx| {
                                this.is_binding = false;
                            })
                            .unwrap();
                        }));
                    }
                }
            )))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.is_binding {
                    this.settings.update(cx, |settings, cx| {
                        settings.input.keybinds.insert(
                            this.serialized_action.json_value(),
                            vec![(event.keystroke.unparse(), this.serialized_action.action())],
                        );
                        cx.notify();
                    });
                    this.binding_timer = None;
                    this.is_binding = false;
                    cx.notify();
                }
            }))
            .on_key_up(cx.listener(|_this, _event: &KeyUpEvent, _window, _cx| {
                // println!("{event:?}")
            }))
            .focusable()
            .track_focus(&self.focus_handle)
    }
}

pub struct Tab {
    pub name: SharedString,
    pub content: AnyElement,
}

impl Tab {
    fn new(name: impl AsRef<str>, content: impl IntoElement) -> Self {
        Self {
            name: name.as_ref().into(),
            content: content.into_any_element(),
        }
    }
}

#[derive(IntoElement)]
pub struct TabBar {
    pub id: ElementId,
    pub tabs: Box<[Tab]>,
}

impl TabBar {
    pub fn new(id: impl Into<ElementId>, tabs: impl IntoIterator<Item = Tab>) -> Self {
        Self {
            id: id.into(),
            tabs: tabs.into_iter().collect(),
        }
    }
}

impl RenderOnce for TabBar {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        let (tab_names, mut tab_content): (Vec<_>, Vec<_>) = self
            .tabs
            .into_iter()
            .map(|Tab { name, content }| (name, content))
            .unzip();

        let current_tab_index = window.use_keyed_state(self.id.clone(), cx, |_window, _cx| 0usize);

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let _lighter_background = theme.palette.lighter_background();
        let _darker_background = theme.palette.darker_background();
        let darkest_background = theme.palette.darkest_background();
        let background = theme.palette.background();
        let _dark_foreground = theme.palette.dark_foreground();
        let _foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        drop(theme);

        div()
            .bg(darkest_background)
            .size_full()
            .flex()
            .flex_col()
            .w_full()
            .items_stretch()
            .child(
                div()
                    .mt_2()
                    .flex()
                    .justify_between()
                    .gap_neg_0p5()
                    .w_full()
                    .children(tab_names.into_iter().enumerate().map(|(idx, name)| {
                        deferred(
                            Button::new(
                                Duration::from_millis(200),
                                (self.id.clone(), name.clone()),
                            )
                            .w_full()
                            .flex()
                            .items_baseline()
                            .justify_center()
                            .px_2()
                            .py_0p5()
                            .border_color(border)
                            .border_1()
                            .rounded_t_lg()
                            .flex_1()
                            .child(name)
                            .when(idx == 0, |this| this.border_l_0().rounded_tl_none())
                            .when(idx == tab_content.len() - 1, |this| {
                                this.border_r_0().rounded_tr_none()
                            })
                            .when_else(
                                idx == *current_tab_index.read(cx),
                                |this| this.background(background).border_b_0(),
                                |this| this.background(darkest_background).border_b_1(),
                            )
                            .on_click(using!([current_tab_index], move |_event, _window, cx| {
                                current_tab_index.write(cx, idx);
                            }))
                            .relative()
                            .top_0p5(),
                        )
                    }))
                    .border_color(background)
                    .border_b_1(),
            )
            .child(
                div()
                    .bg(background)
                    .flex_1()
                    .flex_basis(relative(1.0))
                    .when(!tab_content.is_empty(), |this| {
                        this.child(
                            tab_content.swap_remove(
                                (*current_tab_index.read(cx)).min(tab_content.len() - 1),
                            ),
                        )
                    }),
            )
    }
}
