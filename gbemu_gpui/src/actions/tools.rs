use crate::actions_with_attr;
use gpui::*;

actions_with_attr!(
    tools,
    #[derive(serde::Serialize)],
    [
        Settings,
        ToggleTileViewer,
        ToggleTilemapViewer,
        ToggleMemoryViewer,
        ToggleDebugger,
    ]
);
