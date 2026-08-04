use dioxus_native::prelude::*;
use winit_core::window::ResizeDirection;

#[derive(Props, Clone, PartialEq)]
pub(crate) struct RootProps {
    #[props(extends = GlobalAttributes)]
    pub(crate) attributes: Vec<Attribute>,
    pub(crate) children: Element,
}

#[component]
pub(crate) fn Root(props: RootProps) -> Element {
    let mut is_maximized = use_signal(|| false);

    dioxus_native::use_window_event(move |window_event, _event_loop| match window_event {
        winit_core::event::WindowEvent::SurfaceResized(_physical_size) => {
            is_maximized.set(dioxus_native::use_window().is_maximized());
        }
        _ => {}
    });
    rsx! {
        div {
            class: "size-full flex justify-between",
            class: if !is_maximized() { "pb-10 pl-5 pr-5" } else { "p-0" },
            div { class: "shadow-md shadow-black/30 overflow-hidden rounded-sm flex flex-col grow",
                div {
                    class: "absolute -right-1 opacity-0 bg-red-500 h-full w-2 cursor-e-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::East);
                    },
                }
                div {
                    class: "absolute -left-1 opacity-0 bg-red-500 h-full w-2 cursor-w-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::West);
                    },
                }
                div {
                    class: "absolute -top-1 opacity-0 bg-red-500 w-full h-2 cursor-n-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::North);
                    },
                }
                div {
                    class: "absolute -bottom-1 opacity-0 bg-red-500 w-full h-2 cursor-s-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::South);
                    },
                }
                div {
                    class: "absolute -top-1 -right-1 opacity-0 bg-red-500 w-2.5 h-2.5 cursor-ne-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::NorthEast);
                    },
                }
                div {
                    class: "absolute -top-1 -left-1 opacity-0 bg-red-500 w-2.5 h-2.5 cursor-nw-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::NorthWest);
                    },
                }
                div {
                    class: "absolute -bottom-1 -right-1 opacity-0 bg-red-500 w-2.5 h-2.5 cursor-se-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::SouthEast);
                    },
                }
                div {
                    class: "absolute -bottom-1 -left-1 opacity-0 bg-red-500 w-2.5 h-2.5 cursor-sw-resize",
                    onmousedown: |_| {
                        let window = dioxus_native::use_window();
                        let _ = window.drag_resize_window(ResizeDirection::SouthWest);
                    },
                }
                {props.children}
            }
        }
    }
}
