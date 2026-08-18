use std::path::PathBuf;

use crate::actions_with_attr;
use gpui::*;
use serde::Serialize;

actions_with_attr!(file, #[derive(Serialize)], [RefreshWindow, OpenRom, CloseRom, ToggleFastBoot, Exit]);

#[derive(Clone, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema, Action)]
#[action(namespace = file)]
pub struct OpenRomPath(pub PathBuf);
