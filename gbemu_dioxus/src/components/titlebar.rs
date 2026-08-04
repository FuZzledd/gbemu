use std::sync::Arc;

use blitz_traits::shell::ShellProvider;
use dioxus_elements::input_data::MouseButton;
use dioxus_free_icons::{
    Icon,
    icons::{
        fa_regular_icons::{FaWindowMaximize, FaWindowMinimize, FaWindowRestore},
        md_content_icons::MdClear,
    },
};
use dioxus_native::{
    WindowAttributes,
    prelude::{dioxus_core::Runtime, *},
    use_window_event,
};
use tap::Pipe;
use uzi::using;
use winit::dpi::LogicalPosition;

use crate::EXIT_REQUESTED;
#[derive(Props, Clone, PartialEq)]
pub(crate) struct TitleBarProps {
    #[props(extends = GlobalAttributes)]
    pub(crate) div_attributes: Vec<Attribute>,
}

#[component]
pub(crate) fn TitleBar(props: TitleBarProps) -> Element {
    println!("{:?}", Runtime::current().current_scope_id());

    let window = dioxus_native::use_window();

    let mut is_maximized = use_signal(|| false);
    let shell_provider = use_context::<Arc<dyn ShellProvider>>();

    dioxus_native::use_window_event(using!([window], move |window_event, _event_loop| {
        if let winit_core::event::WindowEvent::SurfaceResized(_physical_size) = window_event {
            is_maximized.set(window.is_maximized());
        }
    }));

    let close_popup = use_signal(|| false);

    let raw_window_handle = dioxus_native::use_raw_window_handle();

    rsx! {
        div {
            class: "h-10 select-none grid grid-cols-[1fr_auto_1fr] content-stretch items-center shrink-0",
            background: "gray",
            onmousedown: {
                using!(
                    [window], move | ev : Event < MouseData >| { let data = ev.data(); if let
                    Some(mouse_button) = data.trigger_button() { match mouse_button {
                    MouseButton::Primary => { let _ = window.drag_window(); }
                    MouseButton::Secondary => { let pos = data.client_coordinates(); window
                    .show_window_menu(winit::dpi::LogicalPosition::new(pos.x, pos.y).into(),); }
                    _ => {} } } }
                )
            },
            ondoubleclick: using!([window], move | _ | { window.clone().set_maximized(! is_maximized()); }),
            ..props.div_attributes,
            div { class: "p-1 flex items-start select-none",
                button {
                    class: "p-0.5 pl-2 pr-2 bg-gray-600",
                    onmousedown: {
                        using!(
                            [raw_window_handle], move | ev : Event < MouseData > | { let
                            window_attributes = unsafe { WindowAttributes::default()
                            .with_transparent(true).with_decorations(false)
                            .with_parent_window(Some(dioxus_native::use_raw_window_handle()))
                            .with_position(ev.screen_coordinates().pipe(| coords |
                            LogicalPosition::new(coords.x, coords.y)),) }; let window_config =
                            dioxus_native::create_window_config(Popup, PopupProps { text : "Test"
                            .to_string(), close : close_popup.into(), }, vec![],
                            vec![Box::new(window_attributes) as _],);
                            dioxus_native::add_window(window_config); }
                        )
                    },
                    onclick: move |ev| {},
                    "File"
                }
            }
            div { class: "justify-self-center select-none", {dioxus_native::use_window().title()} }
            div { class: "p-0 h-5/10 flex mr-2 justify-self-end gap-1",
                button {
                    class: "aspect-square h-full bg-gray-600 rounded-sm hover:bg-gray-500 active:bg-gray-700",
                    onmousedown: |e| {
                        e.stop_propagation();
                    },
                    onclick: using!([window], move | _ | { window.set_minimized(true); }),
                    Icon {
                        width: 15,
                        height: 15,
                        fill: "black",
                        icon: FaWindowMinimize,
                    }
                }
                button {
                    class: "aspect-square h-full bg-gray-600 rounded-sm hover:bg-gray-500 active:bg-gray-700",
                    onmousedown: |e| {
                        e.stop_propagation();
                    },
                    onclick: using!(
                        [window], move | _ | { let window = dioxus_native::use_window(); window
                        .set_maximized(! is_maximized()); }
                    ),
                    if is_maximized() {
                        Icon {
                            width: 15,
                            height: 15,
                            fill: "black",
                            icon: FaWindowRestore,
                        }
                    } else {
                        Icon {
                            width: 15,
                            height: 15,
                            fill: "black",
                            icon: FaWindowMaximize,
                        }
                    }
                }
                button {
                    class: "aspect-square h-full bg-gray-600 rounded-sm hover:bg-gray-500 active:bg-gray-700",
                    onmousedown: |e| {
                        e.stop_propagation();
                    },
                    onclick: move |_| {
                        shell_provider.request_window_close();
                    },
                    Icon {
                        width: 20,
                        height: 20,
                        fill: "black",
                        icon: MdClear,
                    }
                }
            }
        }
    }
}

#[component]
fn Popup(text: String, close: ReadSignal<bool>) -> Element {
    println!("{:?}", Runtime::current().current_scope_id());
    use_window_event(move |_, event_loop| {
        if close() {
            event_loop.exit();
        }
    });

    rsx! {
        div { background: "gray", height: "500px", width: "200px", "hello" }
    }
}
