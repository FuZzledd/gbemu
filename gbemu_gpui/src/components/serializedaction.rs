use gpui::Action;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

#[macro_export]
macro_rules! binds {
    ($(($display:expr, $action:expr)),* $(,)?) => {
        {
            // Import the trait inside the macro expansion
            use $crate::components::serializedaction::SerializableAction;
            vec![$(( $display, $action.to_serialized() )),*]
        }
    }
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

// Manual Debug implementation for SerializedAction
impl fmt::Debug for SerializedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SerializedAction")
            .field("value", &self.0)
            .field("action_name", &self.1.name())
            .finish()
    }
}

// Manual PartialEq implementation for SerializedAction
impl PartialEq for SerializedAction {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1.name() == other.1.name()
    }
}

impl Eq for SerializedAction {}

impl SerializedAction {
    pub fn json_value(&self) -> Value {
        self.0.clone()
    }

    pub fn action(&self) -> Box<dyn Action> {
        self.1.boxed_clone()
    }
}
