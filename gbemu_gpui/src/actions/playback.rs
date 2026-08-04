use crate::actions_with_attr;
use gpui::*;

actions_with_attr!(
    playback,
    #[derive(serde::Serialize)],
    [
        TogglePause,
        StepTick,
        StepFrame,
        FastForward,
        ToggleFastForward
    ]
);
