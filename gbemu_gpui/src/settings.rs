use crate::components::{Checkbox, Separator, radio::RadioButton, root::Root};
use crate::{APP, WindowMap, WindowType};
use crate::{EtceteraStrategy, components::titlebar::TitleBar};
use crate::{
    actions,
    components::{
        button::Button,
        dropdown::Dropdown,
        root::CloseRequestEvent,
        scrollbar::{ListScrollbar, Scrollbar},
    },
};
use crate::{reload_settings, theme::ThemeRegistry};
use better_default::Default;
use convert_case::Casing;
use derive_more::Display;
use etcetera::AppStrategy;
use gbemu_common::theme::{Color, Theme};
use gbemu_core::Palette;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use itertools::Itertools;
use palette::{Srgba, rgb::Rgba};
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
use strum::{EnumDiscriminants, EnumIter, EnumProperty, IntoDiscriminant, IntoEnumIterator};
use tap::{Conv, Tap};
use uzi::using;

#[macro_export]
macro_rules! binds {
    ($(($display:expr, $action:expr)),* $(,)?) => {
        vec![$(( $display, $action.to_serialized() )),*]
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub(crate) video: VideoSettings,
    pub(crate) input: InputSettings,
    pub(crate) emulator: EmulatorSettings,
}
impl Global for Settings {}

#[derive(
    Default,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    EnumIter,
    EnumProperty,
    EnumDiscriminants,
)]
pub enum PaletteOptions {
    #[default]
    #[strum(props(Display = "Grayscale"))]
    Grayscale,
    #[strum(props(Display = "DMG"))]
    Dmg,
    #[strum(props(Display = "Game Boy Pocket"))]
    Pocket,
    #[strum(props(Display = "Game Boy Light"))]
    Light,
    #[strum(props(Display = "Custom"))]
    Custom(Palette),
}

impl From<PaletteOptions> for Palette {
    fn from(value: PaletteOptions) -> Self {
        match value {
            PaletteOptions::Grayscale => Palette::default(),
            PaletteOptions::Dmg => {
                Palette::from([[117, 152, 51], [88, 143, 81], [59, 117, 96], [46, 97, 90]])
            }
            PaletteOptions::Pocket => Palette::from([
                [173, 191, 146],
                [150, 166, 124],
                [114, 126, 100],
                [90, 99, 92],
            ]),
            PaletteOptions::Light => {
                Palette::from([[0, 181, 129], [0, 154, 113], [0, 105, 74], [0, 79, 59]])
            }
            PaletteOptions::Custom(palette) => palette,
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]

pub struct VideoSettings {
    pub color_palette: PaletteOptions,
    pub custom_palette: Palette,
    #[default(true)]
    pub integer_scaling: bool,
    #[default(true)]
    pub fit_window: bool,
    #[default(3)]
    pub scale: u32,
    #[default(false)]
    pub filtering: bool,
    #[default(false)]
    pub show_fps: bool,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct InputSettings {
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
#[serde(default)]
pub struct EmulatorSettings {
    #[default("Catppuccin Frappe".into())]
    pub theme: Cow<'static, str>,
    pub library_path: PathBuf,
    pub bootrom_path: Option<PathBuf>,
    #[default(true)]
    pub fast_boot: bool,
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
    pub emulator: AnyView,
    pub video: AnyView,
    focus: FocusHandle,
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
                window_min_size: Some(size(px(800.0), px(400.0))),
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
        let video = AnyView::from(VideoSettingsTab::new(settings.clone(), cx));

        let entity = cx.new(|cx| Self {
            settings,
            input,
            emulator,
            video,
            focus: cx.focus_handle(),
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

        let _settings_observer = window.use_keyed_state(
            (element_id.clone(), "settings_observer"),
            cx,
            |window, cx| {
                cx.observe(
                    &self.settings,
                    using!([unsaved_changes], move |_this, settings, cx| {
                        unsaved_changes.write(cx, settings.read(cx) != cx.global::<Settings>());
                    }),
                )
            },
        );

        div()
            .track_focus(&self.focus)
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
                    Tab::new("Video", self.video.clone()),
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

                                                reload_settings(cx);

                                                unsaved_changes.write(cx, false);
                                            }
                                        },
                                    ))
                                }),
                                |this| this.disabled(),
                            ),
                    ),
            )
            .on_action::<actions::dev::ToggleInspector>(|_, window, cx| {
                #[cfg(debug_assertions)]
                window.toggle_inspector(cx);
            })
    }
}

pub struct VideoSettingsTab {
    settings: Entity<Settings>,
    palette_dropdown: Entity<Dropdown<PaletteOptions>>,
}

impl VideoSettingsTab {
    pub fn new(settings: Entity<Settings>, cx: &mut App) -> Entity<Self> {
        let custom_palette = settings.read(cx).video.custom_palette;

        let options = PaletteOptions::iter().map(|option| {
            (
                Cow::from(option.get_str("Display").unwrap()),
                match option {
                    PaletteOptions::Custom(_) => PaletteOptions::Custom(custom_palette),
                    other => other,
                },
            )
        });

        let elem = cx.new(|cx| {
            let elem = Self {
                palette_dropdown: cx.new(|cx| {
                    Dropdown::new_raw(options, cx).tap_mut(|this| {
                        this.selected_idx = this
                            .options
                            .iter()
                            .find_position(|item| {
                                item.1.discriminant()
                                    == settings.read(cx).video.color_palette.discriminant()
                            })
                            .map(|(idx, _)| idx)
                            .unwrap_or(0);
                    })
                }),
                settings,
            };

            cx.observe(
                &elem.palette_dropdown,
                |this: &mut VideoSettingsTab, entity, cx| {
                    this.settings.update(cx, |settings, cx| {
                        settings.video.color_palette = entity.read(cx).get_selected().1;
                        cx.notify();
                    });
                },
            )
            .detach();

            elem
        });

        elem
    }
}

impl Render for VideoSettingsTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let element_id = ElementId::from(("VideoSettings", cx.entity_id()));

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let foreground = theme.palette.foreground();
        let background = theme.palette.background();
        let border = theme.palette.gray();

        drop(theme);

        let settings = &self.settings.read(cx).video.clone();

        div()
            .bg(background)
            .text_color(foreground)
            .mt_2()
            .p_5()
            .flex()
            .flex_col()
            .items_stretch()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .child("Game Boy Colour Palette")
                    .child(
                        div()
                            .ml_2()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .items_stretch()
                            .child(self.palette_dropdown.clone())
                            .child(div().flex().gap_2().children({
                                let palette = self.palette_dropdown.read(cx).get_selected().1;

                                let palette_colors: [Color; 4] = Palette::from(palette).into();
                                palette_colors.into_iter().enumerate().map(using!(
                                    [&cx, element_id],
                                    move |(idx, color)| {
                                        Button::new(
                                            Duration::from_millis(100),
                                            (element_id.clone(), format!("palette button {idx}")),
                                        )
                                        .on_click(cx.listener(move |this, event, window, cx| {
                                            let (r, g, b, _) = Srgba::from(color)
                                                .into_format::<_, u8>()
                                                .into_components();
                                            let color = light_file_dialog::color_chooser(
                                                Some("Pick a color"),
                                                None,
                                                Some(light_file_dialog::types::RgbColor {
                                                    r,
                                                    g,
                                                    b,
                                                }),
                                            );

                                            if let Some((
                                                _,
                                                light_file_dialog::RgbColor { r, g, b },
                                            )) = color
                                            {
                                                this.settings.update(cx, |settings, cx| {
                                                    settings.video.custom_palette[idx].r = r;
                                                    settings.video.custom_palette[idx].g = g;
                                                    settings.video.custom_palette[idx].b = b;
                                                    settings.video.custom_palette[idx].a = 0xFF;

                                                    cx.notify();
                                                });
                                                this.palette_dropdown.update(cx, |dropdown, cx| {
                                                    dropdown.options = PaletteOptions::iter()
                                                        .map(|option| {
                                                            (
                                                                Cow::from(
                                                                    option
                                                                        .get_str("Display")
                                                                        .unwrap(),
                                                                ),
                                                                match option {
                                                                    PaletteOptions::Custom(_) => {
                                                                        PaletteOptions::Custom(
                                                                            this.settings
                                                                                .read(cx)
                                                                                .video
                                                                                .custom_palette,
                                                                        )
                                                                    }
                                                                    other => other,
                                                                },
                                                            )
                                                        })
                                                        .collect();
                                                });
                                                cx.notify();
                                            }
                                        }))
                                        .when(
                                            !matches!(palette, PaletteOptions::Custom(_)),
                                            |this| this.disabled(),
                                        )
                                        .flex_1()
                                        .flex_basis(relative(1.0))
                                        .p_1()
                                        .border_color(border)
                                        .border_1()
                                        .rounded_md()
                                        .child(div().rounded_md().w_full().h_8().bg(color))
                                    }
                                ))
                            })),
                    ),
            )
            .child(Separator::new())
            .child(div().flex().justify_center().child("Default Video Options"))
            .child(
                div()
                    .flex()
                    .flex_auto()
                    .mt_2()
                    .justify_center()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .flex_shrink_0()
                            .gap_2()
                            .items_center()
                            .justify_center()
                            .child("Bilinear Filtering")
                            .child(
                                Checkbox::new(
                                    settings.filtering,
                                    (element_id.clone(), "filtering"),
                                )
                                .on_checked(cx.listener(using!(
                                    [self.settings],
                                    move |this, &checked, window, cx| {
                                        settings.update(cx, |settings, cx| {
                                            settings.video.filtering = checked;
                                            cx.notify();
                                        });
                                        cx.notify();
                                    }
                                ))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .flex_shrink_0()
                            .gap_2()
                            .items_center()
                            .justify_center()
                            .child("Resize to Fit Window")
                            .child(
                                Checkbox::new(
                                    settings.fit_window,
                                    (element_id.clone(), "fit_window"),
                                )
                                .on_checked(cx.listener(using!(
                                    [self.settings],
                                    move |this, &checked, window, cx| {
                                        settings.update(cx, |settings, cx| {
                                            settings.video.fit_window = checked;
                                            cx.notify();
                                        });
                                        cx.notify();
                                    }
                                ))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .flex_shrink_0()
                            .gap_2()
                            .items_center()
                            .justify_center()
                            .child("Force Integer Scaling")
                            .child(
                                Checkbox::new(
                                    settings.integer_scaling,
                                    (element_id.clone(), "integer_scaling"),
                                )
                                .on_checked(cx.listener(using!(
                                    [self.settings],
                                    move |this, &checked, window, cx| {
                                        settings.update(cx, |settings, cx| {
                                            settings.video.integer_scaling = checked;
                                            cx.notify();
                                        });
                                        cx.notify();
                                    }
                                ))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .flex_shrink_0()
                            .gap_2()
                            .items_center()
                            .justify_center()
                            .child("Show FPS")
                            .child(
                                Checkbox::new(settings.show_fps, (element_id.clone(), "show_fps"))
                                    .on_checked(cx.listener(using!(
                                        [self.settings],
                                        move |this, &checked, window, cx| {
                                            settings.update(cx, |settings, cx| {
                                                settings.video.show_fps = checked;
                                                cx.notify();
                                            });
                                            cx.notify();
                                        }
                                    ))),
                            ),
                    ),
            )
            .child(div().flex().mt_1().gap_2().child("Fixed Scale"))
            .child(div().flex().children((1..9).map(|scale| {
                div()
                    .flex()
                    .flex_1()
                    .flex_shrink_0()
                    .gap_2()
                    .items_center()
                    .justify_center()
                    .child(format!("{scale}x"))
                    .child(
                        RadioButton::new(
                            settings.scale == scale,
                            (element_id.clone(), format!("scale-{scale}x")),
                        )
                        .on_checked(cx.listener(using!(
                            [self.settings],
                            move |this, &_checked, window, cx| {
                                settings.update(cx, |settings, cx| {
                                    settings.video.scale = scale;
                                    cx.notify();
                                });
                                cx.notify();
                            }
                        ))),
                    )
            })))
    }
}

pub struct EmulatorSettingsTab {
    settings: Entity<Settings>,
    library_path_state: Entity<EditableTextState>,
    theme_dropdown: Entity<Dropdown<Theme>>,
}

impl EmulatorSettingsTab {
    pub fn new(settings: Entity<Settings>, cx: &mut App) -> Entity<Self> {
        let options = cx.global::<ThemeRegistry>().themes().clone();

        let elem = cx.new(|cx| {
            let elem = Self {
                library_path_state: cx.new(|cx| {
                    EditableTextState::new(
                        StringStorage::from(
                            settings.read(cx).emulator.library_path.to_string_lossy(),
                        ),
                        cx,
                    )
                }),
                theme_dropdown: cx.new(|cx| {
                    Dropdown::new_raw(options, cx).tap_mut(|this| {
                        this.selected_idx = this
                            .options
                            .iter()
                            .find_position(|item| item.0 == settings.read(cx).emulator.theme)
                            .unwrap()
                            .0;
                    })
                }),
                settings,
            };

            cx.subscribe(
                &elem.library_path_state,
                |this: &mut EmulatorSettingsTab, state, _: &TextChanged, cx| {
                    this.settings.update(cx, |settings, cx| {
                        settings.emulator.library_path = state.read(cx).as_str().into();
                    })
                },
            )
            .detach();

            cx.observe(
                &elem.theme_dropdown,
                |this: &mut EmulatorSettingsTab, entity, cx| {
                    this.settings.update(cx, |settings, cx| {
                        settings.emulator.theme = entity.read(cx).get_selected().0.clone();
                        cx.notify();
                    })
                },
            )
            .detach();

            elem
        });

        elem
    }
}

impl Render for EmulatorSettingsTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .gap_2()
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
                        .child("Browse")
                        .on_click(cx.listener(
                            |this, event: &ClickEvent, window, cx| {
                                if event.standard_click() {
                                    let result = rfd::FileDialog::new()
                                        .set_can_create_directories(true)
                                        .set_directory(this.library_path_state.read(cx).as_str())
                                        .set_parent(window)
                                        .set_title("Pick a ROM Library folder")
                                        .pick_folder();

                                    if let Some(result) = result {
                                        this.library_path_state.update(cx, |path, cx| {
                                            path.emplace(&result.to_string_lossy(), cx)
                                        });
                                        this.settings.update(cx, |this, cx| {
                                            this.emulator.library_path = result;
                                        })
                                    }
                                }
                            },
                        )),
                    ),
            )
            .child(
                div().flex().items_center().child("Theme").child(
                    div()
                        .ml_2()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_stretch()
                        .child(self.theme_dropdown.clone()),
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
                    using!([self.bind_sections], move |idx, _window, cx| {
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
            .child(ListScrollbar::new(
                ("scrollbar", cx.entity_id()),
                self.list_state.clone(),
            ))
    }
}

pub struct InputSection {
    section_name: &'static str,
    binds: Vec<(&'static str, SerializedAction)>,
    settings: Entity<Settings>,
    resolved_style: Entity<Option<StyleRefinement>>,
}
impl InputSection {
    fn new<T: AppContext>(
        section_name: &'static str,
        binds: Vec<(&'static str, SerializedAction)>,
        settings: Entity<Settings>,
        cx: &mut T,
    ) -> Entity<Self> {
        cx.new(|cx| Self {
            section_name,
            settings,
            binds,
            resolved_style: cx.new(|cx| None),
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

        let mut elem = div()
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
            );

        self.resolved_style.write(cx, Some(elem.style().clone()));
        elem
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

    let mut resolved_style = None;
    let bindable = window.use_keyed_state((element_id, name), cx, |_window, cx| {
        let elem = BindableButton::new(bind.1.clone(), settings, cx);
        resolved_style = Some(elem.resolved_style.clone());
        elem
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
                .items_center()
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
    resolved_style: Entity<StyleRefinement>,
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
            resolved_style: cx.new(|cx| StyleRefinement::default()),
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

        fn refresh_button_text(
            button_text: Entity<String>,
            cx: &mut App,
            this: &mut BindableButton,
        ) {
            button_text.write(
                cx,
                this.settings
                    .read(cx)
                    .input
                    .keybinds
                    .bindings
                    .get(&this.serialized_action.json_value())
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
                    .unwrap_or("None".into()),
            );
        }

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
                [self.focus_handle, button_text],
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
            .on_aux_click(cx.listener(using!(
                [self.focus_handle, button_text],
                move |this, event: &ClickEvent, window, cx| {
                    if event.is_right_click() {
                        this.is_binding = false;
                        this.binding_timer = None;
                        this.settings.update(cx, |settings, cx| {
                            settings
                                .input
                                .keybinds
                                .remove(&this.serialized_action.json_value());
                            cx.notify();
                        });

                        refresh_button_text(button_text.clone(), cx, this);

                        cx.notify();
                    }
                }
            )))
            .on_key_down(cx.listener(using!(
                [button_text],
                move |this, event: &KeyDownEvent, _window, cx| {
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

                        refresh_button_text(button_text.clone(), cx, this);

                        cx.notify();
                    }
                }
            )))
            .on_key_up(cx.listener(|_this, _event: &KeyUpEvent, _window, _cx| {
                // println!("{event:?}")
            }))
            .focusable()
            .track_focus(&self.focus_handle)
            .resolved_style(self.resolved_style.clone())
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
