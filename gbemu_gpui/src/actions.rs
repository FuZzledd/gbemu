pub mod file;
pub mod tools;
pub mod video;

pub mod dev;
pub mod game;
pub mod playback;

#[macro_export]
macro_rules! actions_with_attr {
    ($namespace:path, #[$attr:meta], [$($name:ident),* $(,)?] ) => {
        gpui::actions!($namespace,  [$(#[$attr] $name),*]);
    }
}
