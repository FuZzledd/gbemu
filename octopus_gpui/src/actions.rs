pub mod file;
pub mod help;
pub mod tools;
pub mod video;

pub mod dev;
pub mod game;
pub mod playback;

use core::fmt::Debug;
use gpui::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::APP;

#[macro_export]
macro_rules! actions_with_attr {
    ($namespace:path, #[$attr:meta], [$($name:ident),* $(,)?] ) => {
        gpui::actions!($namespace,  [$(#[$attr] $name),*]);
    }
}

pub trait SerializableAction: Action {
    fn to_serialized(&self) -> SerializedAction;
    fn serializable_boxed_clone(&self) -> Box<dyn SerializableAction>;
}

impl Clone for Box<dyn SerializableAction> {
    fn clone(&self) -> Self {
        self.serializable_boxed_clone()
    }
}

impl<T> SerializableAction for T
where
    T: Action + Serialize + Clone,
{
    fn to_serialized(&self) -> SerializedAction {
        let value = serde_json::to_value(self).unwrap();
        match value {
            Value::Null => SerializedAction(self.name().into(), self.boxed_clone()),
            other => SerializedAction([self.name().into(), other].into(), self.boxed_clone()),
        }
    }

    fn serializable_boxed_clone(&self) -> Box<dyn SerializableAction> {
        Box::new(self.clone())
    }
}

impl Debug for dyn SerializableAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("dyn SerializableAction")
            .field("name", &self.name())
            .finish()
    }
}

#[derive(Debug)]
pub struct SerializedAction(pub Value, pub Box<dyn Action>);

impl Clone for SerializedAction {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.boxed_clone())
    }
}

impl core::hash::Hash for SerializedAction {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for SerializedAction {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1.partial_eq(other.1.as_ref())
    }
}

impl Eq for SerializedAction {

}

impl SerializedAction {
    pub fn json_value(&self) -> Value {
        self.0.clone()
    }

    pub fn action(&self) -> Box<dyn Action> {
        self.1.boxed_clone()
    }
}

impl<T: SerializableAction> From<T> for SerializedAction {
    fn from(value: T) -> Self {
        value.to_serialized()
    }
}

impl Serialize for dyn SerializableAction {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_serialized().serialize(serializer)
    }
}

impl Serialize for SerializedAction {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SerializedAction {
    fn deserialize<D>(deserializer: D) -> std::prelude::v1::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        let action_raw = serde_json::Value::deserialize(deserializer)?;

        APP.with(move |cx| {
            let taken_cx = cx.take();
            let res = {
                let cx = taken_cx
                    .as_ref()
                    .ok_or(de::Error::custom("Couldn't get app context"))?
                    .clone();

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
                
                action
            };
            cx.set(taken_cx);
            Ok(
                SerializedAction(action_raw, res)
            )
        })
    }
}
