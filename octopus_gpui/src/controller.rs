use core::{convert::Infallible, panic::Location, str::FromStr};
use std::{collections::HashMap, rc::Rc, sync::Arc};

use derive_more::{Display, FromStr};
use gilrs::{Axis, Button, Event, GamepadId, Gilrs};
use gpui::*;
use itertools::Itertools;
use serde::{
    Deserialize, Serialize,
    de::{Unexpected, Visitor},
};
use slotmap::{DenseSlotMap, new_key_type};
use smallvec::SmallVec;
use strum::{EnumDiscriminants, IntoDiscriminant};

use crate::actions::SerializedAction;

new_key_type! { pub struct GamepadEventKey; }
new_key_type! { pub struct GamepadPressEventKey; }
new_key_type! { pub struct GamepadReleaseEventKey; }

pub struct GamepadService {
    gilrs: Gilrs,
    pub on_gamepad_event_listeners: DenseSlotMap<
        GamepadEventKey,
        (
            Arc<dyn Fn(&Event, &mut Self, &mut Window, &mut App) + 'static>,
            AnyWindowHandle,
        ),
    >,
    pub on_gamepad_press_event_listeners: DenseSlotMap<
        GamepadPressEventKey,
        (
            Arc<dyn Fn(&GamepadButton, &Event, &mut Self, &mut Window, &mut App) + 'static>,
            AnyWindowHandle,
        ),
    >,
    pub on_gamepad_release_event_listeners: DenseSlotMap<
        GamepadReleaseEventKey,
        (
            Arc<dyn Fn(&GamepadButton, &Event, &mut Self, &mut Window, &mut App) + 'static>,
            AnyWindowHandle,
        ),
    >,
    bindings: Rc<[GamepadBinding]>,
    prev_axis_values: HashMap<Axis, f32>,
}

impl Global for GamepadService {}

impl GamepadService {
    pub fn new() -> Self {
        Self {
            gilrs: Gilrs::new().unwrap(),
            on_gamepad_event_listeners: Default::default(),
            on_gamepad_press_event_listeners: Default::default(),
            on_gamepad_release_event_listeners: Default::default(),
            bindings: Default::default(),
            prev_axis_values: Default::default(),
        }
    }

    pub fn bind_buttons(&mut self, bindings: impl IntoIterator<Item = GamepadBinding>) {
        self.bindings = bindings.into_iter().collect();
    }
    pub fn get_bindings_for_button(&self, button: GamepadButton) -> SmallVec<[GamepadBinding; 1]> {
        self.bindings
            .iter()
            .filter(|binding| binding.combination.contains(&button))
            .cloned()
            .collect()
    }

    pub fn check_combination(
        &mut self,
        id: GamepadId,
        combination: &GamepadButtonCombination,
    ) -> bool {
        let gamepad = self.gilrs.gamepad(id);
        combination.iter().copied().all(|button| match button {
            GamepadButton::Button(button) => gamepad.is_pressed(button),
            GamepadButton::SignedAxis(signed_axis, axis_sign) => {
                let value = gamepad.value(signed_axis.into());
                match axis_sign {
                    AxisSign::Positive => value > 0.6,
                    AxisSign::Negative => value < -0.6,
                }
            }
            GamepadButton::UnsignedAxis(unsigned_axis) => gamepad.value(unsigned_axis.into()) > 0.8,
        })
    }

    fn check_events(&mut self, cx: &mut App) {
        while let Some(event) = self.gilrs.next_event() {
            let input_pressed = match event.event {
                gilrs::EventType::ButtonPressed(button, _) => {
                    for (handler, window_handle) in
                        self.on_gamepad_press_event_listeners.clone().values()
                    {
                        let window_handle = *window_handle;
                        let handler = handler.clone();

                        cx.update_window(window_handle, |_, window, cx| {
                            handler(&GamepadButton::Button(button), &event, self, window, cx);
                        })
                        .unwrap();
                    }
                    true
                }
                gilrs::EventType::ButtonReleased(button, _) => {
                    for (handler, window_handle) in
                        self.on_gamepad_release_event_listeners.clone().values()
                    {
                        let window_handle = *window_handle;
                        let handler = handler.clone();

                        cx.update_window(window_handle, |_, window, cx| {
                            handler(&GamepadButton::Button(button), &event, self, window, cx);
                        })
                        .unwrap();
                    }
                    false
                }
                gilrs::EventType::AxisChanged(axis, value, _) => {
                    let prev = self.prev_axis_values.entry(axis).or_default();
                    let pressed = match axis {
                        Axis::LeftZ | Axis::RightZ => *prev < 0.8 && value > 0.8,
                        _ => prev.abs() < 0.6 && value.abs() > 0.6,
                    };
                    let released = match axis {
                        Axis::LeftZ | Axis::RightZ => *prev > 0.8 && value < 0.8,
                        _ => prev.abs() > 0.6 && value.abs() < 0.6,
                    };
                    *prev = value;
                    let prev = *prev;

                    if pressed {
                        for (handler, window_handle) in
                            self.on_gamepad_press_event_listeners.clone().values()
                        {
                            let window_handle = *window_handle;
                            let handler = handler.clone();

                            let button = match axis {
                                Axis::LeftZ | Axis::RightZ => {
                                    GamepadButton::UnsignedAxis(axis.try_into().unwrap())
                                }
                                _ => {
                                    let sign = if value.is_sign_positive() {
                                        AxisSign::Positive
                                    } else {
                                        AxisSign::Negative
                                    };
                                    GamepadButton::SignedAxis(axis.try_into().unwrap(), sign)
                                }
                            };

                            cx.update_window(window_handle, |_, window, cx| {
                                handler(&button, &event, self, window, cx);
                            })
                            .unwrap();
                        }
                    }
                    if released {
                        for (handler, window_handle) in
                            self.on_gamepad_release_event_listeners.clone().values()
                        {
                            let window_handle = *window_handle;
                            let handler = handler.clone();

                            let button = match axis {
                                Axis::LeftZ | Axis::RightZ => {
                                    GamepadButton::UnsignedAxis(axis.try_into().unwrap())
                                }
                                _ => {
                                    let sign = if prev.is_sign_positive() {
                                        AxisSign::Positive
                                    } else {
                                        AxisSign::Negative
                                    };
                                    GamepadButton::SignedAxis(axis.try_into().unwrap(), sign)
                                }
                            };

                            cx.update_window(window_handle, |_, window, cx| {
                                handler(&button, &event, self, window, cx);
                            })
                            .unwrap();
                        }
                    }
                    pressed
                }
                _ => false,
            };

            if input_pressed {
                for binding in self.bindings.clone().iter() {
                    if self.check_combination(event.id, &binding.combination) {
                        for window in cx.windows() {
                            window
                                .update(cx, |_root, window, cx| {
                                    if let Some(focused) = window.focused(cx) {
                                        focused.dispatch_action(
                                            binding.action().as_ref(),
                                            window,
                                            cx,
                                        );
                                    }
                                })
                                .unwrap();
                        }
                    }
                }
            }

            for (handler, window_handle) in self.on_gamepad_event_listeners.clone().values() {
                let window_handle = *window_handle;
                let handler = handler.clone();

                cx.update_window(window_handle, |_, window, cx| {
                    handler(&event, self, window, cx);
                })
                .unwrap();
            }
        }
        self.gilrs.inc();
    }

    pub fn run_event_loop(cx: &mut App) -> Task<()> {
        cx.default_global::<Self>();

        cx.spawn(async |cx| {
            loop {
                cx.spawn(async |cx| {
                    cx.update_global::<Self, _>(|this, cx| this.check_events(cx));
                })
                .await;
            }
        })
    }
}

impl Default for GamepadService {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: StatefulInteractiveElement> GamepadEventsExt for T {
    #[track_caller]
    fn on_gamepad_event(
        mut self,
        cx: &mut App,
        window: &mut Window,
        callback: impl Fn(&Event, &mut GamepadService, &mut Window, &mut App) + 'static,
    ) -> Self {
        let element_id = self.interactivity().element_id.clone().unwrap();
        let caller_location = Location::caller();
        let element_id = ElementId::from((element_id, caller_location.to_string()));
        let callback = Arc::new(callback);

        let _handle = window.use_keyed_state(element_id, cx, |window, cx| {
            let gamepad_service = cx.global_mut::<GamepadService>();

            let map = &mut gamepad_service.on_gamepad_event_listeners;
            let key = map.insert((callback.clone(), window.window_handle()));

            cx.on_release(|key, cx| {
                cx.global_mut::<GamepadService>()
                    .on_gamepad_event_listeners
                    .remove(*key);
            })
            .detach();

            key
        });

        let key = *_handle.read(cx);
        let gamepad_service = cx.global_mut::<GamepadService>();

        let map = &mut gamepad_service.on_gamepad_event_listeners;
        if let Some(entry) = map.get_mut(key) {
            *entry = (callback, window.window_handle());
        }

        self
    }

    #[track_caller]
    fn on_gamepad_press(
        mut self,
        cx: &mut App,
        window: &mut Window,
        callback: impl Fn(&GamepadButton, &Event, &mut GamepadService, &mut Window, &mut App) + 'static,
    ) -> Self {
        let element_id = self.interactivity().element_id.clone().unwrap();
        let caller_location = Location::caller();
        let element_id = ElementId::from((element_id, caller_location.to_string()));
        let callback = Arc::new(callback);

        let _handle = window.use_keyed_state(element_id, cx, |window, cx| {
            let gamepad_service = cx.global_mut::<GamepadService>();

            let map = &mut gamepad_service.on_gamepad_press_event_listeners;
            let key = map.insert((callback.clone(), window.window_handle()));

            cx.on_release(|key, cx| {
                cx.global_mut::<GamepadService>()
                    .on_gamepad_press_event_listeners
                    .remove(*key);
            })
            .detach();

            key
        });

        let key = *_handle.read(cx);
        let gamepad_service = cx.global_mut::<GamepadService>();

        let map = &mut gamepad_service.on_gamepad_press_event_listeners;
        if let Some(entry) = map.get_mut(key) {
            *entry = (callback, window.window_handle());
        }

        self
    }

    #[track_caller]
    fn on_gamepad_release(
        mut self,
        cx: &mut App,
        window: &mut Window,
        callback: impl Fn(&GamepadButton, &Event, &mut GamepadService, &mut Window, &mut App) + 'static,
    ) -> Self {
        let element_id = self.interactivity().element_id.clone().unwrap();
        let caller_location = Location::caller();
        let element_id = ElementId::from((element_id, caller_location.to_string()));
        let callback = Arc::new(callback);

        let _handle = window.use_keyed_state(element_id, cx, |window, cx| {
            let gamepad_service = cx.global_mut::<GamepadService>();

            let map = &mut gamepad_service.on_gamepad_release_event_listeners;
            let key = map.insert((callback.clone(), window.window_handle()));

            cx.on_release(|key, cx| {
                cx.global_mut::<GamepadService>()
                    .on_gamepad_release_event_listeners
                    .remove(*key);
            })
            .detach();

            key
        });

        let key = *_handle.read(cx);
        let gamepad_service = cx.global_mut::<GamepadService>();

        let map = &mut gamepad_service.on_gamepad_release_event_listeners;
        if let Some(entry) = map.get_mut(key) {
            *entry = (callback, window.window_handle());
        }

        self
    }
}

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, Display, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub enum AxisSign {
    #[display("+")]
    Positive,
    #[display("-")]
    Negative,
}
impl FromStr for AxisSign {
    type Err = ();

    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        match s {
            "+" => Ok(Self::Positive),
            "-" => Ok(Self::Negative),
            _ => Err(()),
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    Display,
    FromStr,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
pub enum SignedAxis {
    LeftStickX = 1,
    LeftStickY = 2,
    RightStickX = 4,
    RightStickY = 5,
    DPadX = 7,
    DPadY = 8,
}

impl From<SignedAxis> for Axis {
    fn from(value: SignedAxis) -> Self {
        use Axis::*;
        match value {
            SignedAxis::LeftStickX => LeftStickX,
            SignedAxis::LeftStickY => LeftStickY,
            SignedAxis::RightStickX => RightStickX,
            SignedAxis::RightStickY => RightStickY,
            SignedAxis::DPadX => DPadX,
            SignedAxis::DPadY => DPadY,
        }
    }
}

impl TryFrom<Axis> for SignedAxis {
    type Error = ();

    fn try_from(value: Axis) -> std::prelude::v1::Result<Self, Self::Error> {
        use SignedAxis::*;
        Ok(match value {
            Axis::LeftZ | Axis::RightZ | Axis::Unknown => Err(())?,
            Axis::LeftStickX => LeftStickX,
            Axis::LeftStickY => LeftStickY,
            Axis::RightStickX => RightStickX,
            Axis::RightStickY => RightStickY,
            Axis::DPadX => DPadX,
            Axis::DPadY => DPadY,
        })
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    Display,
    FromStr,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
pub enum UnsignedAxis {
    LeftZ = 3,
    RightZ = 6,
}

impl From<UnsignedAxis> for Axis {
    fn from(value: UnsignedAxis) -> Self {
        use Axis::*;
        match value {
            UnsignedAxis::LeftZ => LeftZ,
            UnsignedAxis::RightZ => RightZ,
        }
    }
}

impl TryFrom<Axis> for UnsignedAxis {
    type Error = ();

    fn try_from(value: Axis) -> std::prelude::v1::Result<Self, Self::Error> {
        use UnsignedAxis::*;
        Ok(match value {
            Axis::LeftZ => LeftZ,
            Axis::RightZ => RightZ,
            _ => Err(())?,
        })
    }
}

#[derive(Copy, Clone, Display, PartialEq, Eq, Hash, EnumDiscriminants, Debug)]
#[strum_discriminants(derive(PartialOrd, Ord))]
pub enum GamepadButton {
    #[display("{}", serde_json::to_value(_0).unwrap().as_str().unwrap())]
    Button(Button),
    #[display("{_1}{_0}")]
    SignedAxis(SignedAxis, AxisSign),
    UnsignedAxis(UnsignedAxis),
}

impl PartialOrd for GamepadButton {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GamepadButton {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.discriminant().cmp(&other.discriminant()) {
            core::cmp::Ordering::Equal => self.to_string().cmp(&other.to_string()),
            other => other,
        }
    }
}

impl Serialize for GamepadButton {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for GamepadButton {
    fn deserialize<D>(deserializer: D) -> std::prelude::v1::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct GamepadButtonVisitor;
        impl<'de> Visitor<'de> for GamepadButtonVisitor {
            type Value = GamepadButton;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(
                    formatter,
                    "Expected a string containing a valid gamepad input"
                )
            }

            fn visit_str<E>(self, v: &str) -> std::prelude::v1::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                v.parse()
                    .map_err(|_| serde::de::Error::invalid_value(Unexpected::Str(v), &self))
            }
        }

        deserializer.deserialize_str(GamepadButtonVisitor)
    }
}

impl FromStr for GamepadButton {
    type Err = Infallible;

    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        if let Ok(button) = serde_json::from_value(serde_json::to_value(s).unwrap()) {
            Ok(GamepadButton::Button(button))
        } else if let Some((sign, axis)) = s.split_at_checked(1)
            && let Ok(sign) = sign.parse()
            && let Ok(axis) = axis.parse()
        {
            Ok(GamepadButton::SignedAxis(axis, sign))
        } else if let Ok(axis) = s.parse() {
            Ok(GamepadButton::UnsignedAxis(axis))
        } else {
            Ok(GamepadButton::Button(Button::Unknown))
        }
    }
}

#[derive(Display, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SmallVec<[GamepadButton; 1]>")]
#[repr(transparent)]
#[display("{}", &_0.iter().join(" + "))]
pub struct GamepadButtonCombination(SmallVec<[GamepadButton; 1]>);

impl core::ops::Deref for GamepadButtonCombination {
    type Target = SmallVec<[GamepadButton; 1]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: IntoIterator<Item = GamepadButton>> From<T> for GamepadButtonCombination {
    fn from(value: T) -> Self {
        Self(value.into_iter().sorted().dedup().collect())
    }
}

impl From<GamepadButton> for GamepadButtonCombination {
    fn from(value: GamepadButton) -> Self {
        Self(SmallVec::from([value]))
    }
}

impl GamepadButtonCombination {
    pub fn new(buttons: impl IntoIterator<Item = GamepadButton>) -> Self {
        let mut buttons: SmallVec<[GamepadButton; 1]> = buttons.into_iter().collect();
        buttons.sort();
        Self(buttons)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, GamepadButton> {
        self.0.iter()
    }
}

#[derive(Clone, Debug)]
pub struct GamepadBinding {
    combination: GamepadButtonCombination,
    action: SerializedAction,
}

impl GamepadBinding {
    pub fn new(
        buttons: impl Into<GamepadButtonCombination>,
        action: impl Into<SerializedAction>,
    ) -> Self {
        Self {
            combination: buttons.into(),
            action: action.into(),
        }
    }

    pub fn combination(&self) -> &GamepadButtonCombination {
        &self.combination
    }
    pub fn action(&self) -> Box<dyn Action> {
        self.action.action()
    }
}

pub trait GamepadEventsExt {
    fn on_gamepad_event(
        self,
        cx: &mut App,
        window: &mut Window,
        callback: impl Fn(&Event, &mut GamepadService, &mut Window, &mut App) + 'static,
    ) -> Self;

    fn on_gamepad_press(
        self,
        cx: &mut App,
        window: &mut Window,
        callback: impl Fn(&GamepadButton, &Event, &mut GamepadService, &mut Window, &mut App) + 'static,
    ) -> Self;

    fn on_gamepad_release(
        self,
        cx: &mut App,
        window: &mut Window,
        callback: impl Fn(&GamepadButton, &Event, &mut GamepadService, &mut Window, &mut App) + 'static,
    ) -> Self;
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::rust_2015::test;

    #[test]
    fn test_gamepad_serialize() {
        let button_combo = GamepadButtonCombination::from([
            GamepadButton::Button(Button::North),
            GamepadButton::Button(Button::LeftTrigger),
            GamepadButton::SignedAxis(SignedAxis::LeftStickX, AxisSign::Positive),
            GamepadButton::UnsignedAxis(UnsignedAxis::LeftZ),
            GamepadButton::Button(Button::Unknown),
        ]);

        let serialized = serde_json::to_string(&button_combo).unwrap();
        assert_eq!(
            serialized,
            r#"["LeftTrigger","North","Unknown","+LeftStickX","LeftZ"]"#
        );
    }

    #[test]
    fn test_gamepad_deserialize() {
        let button_combo = GamepadButtonCombination::from([
            GamepadButton::Button(Button::North),
            GamepadButton::Button(Button::LeftTrigger),
            GamepadButton::SignedAxis(SignedAxis::LeftStickX, AxisSign::Positive),
            GamepadButton::UnsignedAxis(UnsignedAxis::LeftZ),
            GamepadButton::Button(Button::Unknown),
        ]);

        let serialized = r#"["LeftTrigger","North","+LeftStickX","LeftZ","Unknown","Greg"]"#;
        let deserialized = serde_json::from_str(serialized).unwrap();
        assert_eq!(button_combo, deserialized);
    }
}
