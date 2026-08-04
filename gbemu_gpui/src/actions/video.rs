use crate::actions_with_attr;
use gpui::{Action, actions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

actions_with_attr!(
    video,
    #[derive(serde::Serialize)],
    [
        ToggleFullscreen,
        ToggleIntegerScaling,
        ToggleFixedSize,
        ToggleLinearFiltering,
        ToggleShowFps
    ]
);

#[derive(Action, PartialEq, Hash, Clone, JsonSchema, Deserialize, Serialize, Copy)]
#[action(namespace = video)]
pub struct ToggleScaleFactor(pub u32);
