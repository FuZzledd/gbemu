use crate::actions_with_attr;
use gpui::*;

actions_with_attr!(game, #[derive(serde::Serialize)], [Up, Down, Left, Right, B, A, Start, Select]);
