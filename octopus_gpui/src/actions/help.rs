use gpui::*;

use crate::actions_with_attr;

actions_with_attr!(help, #[derive(serde::Serialize)], [OpenAbout, Luna]);
