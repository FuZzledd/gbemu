use core::time::Duration;

use convert_case::Casing;
use gpui::{prelude::*, *};
use itertools::Itertools;
use tap::Tap;
use uzi::using;

use crate::{components::button::Button, theme::ThemeRegistry};

pub struct MenuBar {
    bounds: Bounds<Pixels>,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            bounds: Default::default(),
        }
    }
}

actions!(menubar, [CloseMenus]);

type OpenedMenuPopup = (Bounds<Pixels>, OwnedMenu);

impl Render for MenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme().clone();

        let background = theme.palette.darker_background();

        let hover_background = theme.palette.background();

        let foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        let app_menu = window.use_state(cx, |_, cx| cx.get_menus().unwrap_or_default());

        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("menubar", entity_id));

        let current_popup: Entity<Option<(Bounds<Pixels>, OwnedMenu)>> =
            window.use_state(cx, |window, cx| None);

        let entity = cx.entity();

        let menu_bounds: Entity<Vec<Bounds<Pixels>>> =
            window.use_keyed_state((element_id.clone(), "total_bounds"), cx, |_, _| vec![]);

        {
            let menus = cx.get_menus().unwrap_or_default();

            app_menu.write(cx, menus.clone());

            if let Some((_, ref mut current_menu)) = *current_popup.as_mut(cx)
                && let Some(menu) = menus
                    .into_iter()
                    .find(|menu| menu.name == current_menu.name)
            {
                current_menu.items = menu.items;
            }
        }

        let menus = app_menu.read(cx).clone().into_iter().map(|menu| {
            let id = ElementId::from((element_id.clone(), menu.name.clone()));

            MenuBarButton::new(id, menu, current_popup.clone())
        });

        cx.subscribe_self(using!(
            [current_popup],
            move |this, _: &DismissEvent, cx| {
                current_popup.write(cx, None);
            }
        ))
        .detach();

        let viewport_size = window.viewport_size();

        div()
            .on_mouse_down_out(using!([menu_bounds, entity], move |event, window, cx| {
                let mouse_pos = event.position;

                if menu_bounds
                    .read(cx)
                    .iter()
                    .all(|bounds| !bounds.contains(&mouse_pos))
                {
                    entity.update(cx, |this, cx| {
                        cx.emit(DismissEvent);
                    })
                }
            }))
            .relative()
            .bg(background)
            .pl_1()
            .pr_1()
            .shadow_sm()
            .border_b_1()
            .border_color(border)
            .text_color(foreground)
            .w_full()
            .flex()
            .justify_start()
            .items_center()
            .gap_0p5()
            .children(menus)
            .when_some(
                current_popup.read(cx).clone(),
                using!(
                    [element_id, current_popup, menu_bounds],
                    move |this, (bounds, menu)| {
                        this.child(deferred(
                            anchored()
                                .position_mode(AnchoredPositionMode::Window)
                                .snap_to_window_with_margin(px(16.0))
                                .anchor(Anchor::TopLeft)
                                .position(bounds.bottom_left())
                                .child(
                                    MenuPopup::new(
                                        (element_id, menu.name.clone().as_ref()),
                                        menu,
                                        current_popup,
                                        menu_bounds,
                                    )
                                    .max_w(viewport_size.width * 0.8),
                                ),
                        ))
                    }
                ),
            )
            .on_bounds_prepaint(using!([entity, menu_bounds], move |bounds, _, cx| {
                entity.update(cx, |this, cx| {
                    this.bounds = bounds;
                    menu_bounds.as_mut(cx).clear();
                })
            }))
    }
}

impl<T: 'static> EventEmitter<T> for MenuBar {}

#[derive(IntoElement)]
struct MenuBarButton {
    id: ElementId,
    menu: OwnedMenu,
    base: Stateful<Div>,
    current_root_menu: Entity<Option<OpenedMenuPopup>>,
}

impl MenuBarButton {
    fn new(
        id: impl Into<ElementId>,
        menu: OwnedMenu,
        current_root_menu: Entity<Option<OpenedMenuPopup>>,
    ) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            menu,
            base: div().id((id, "container")),
            current_root_menu,
        }
    }
}

impl InteractiveElement for MenuBarButton {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for MenuBarButton {}

impl RenderOnce for MenuBarButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let bounds =
            window.use_keyed_state((self.id.clone(), "bounds_reporter"), cx, |window, cx| {
                Bounds::default()
            });

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.darker_background();

        let hover_background = theme.palette.background();

        let foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        drop(theme);

        self.base
            .flex()
            .items_center()
            .h_full()
            .child(
                Button::new(Duration::from_millis(200), self.id.clone())
                    .background(background)
                    .hover_background(hover_background)
                    .h(relative(0.8))
                    .p_0p5()
                    .pl_1()
                    .pr_1()
                    .flex()
                    .justify_center()
                    .items_center()
                    .rounded_sm()
                    .child(self.menu.name.clone())
                    .on_mouse_down(
                        MouseButton::Left,
                        using!(
                            [self.menu, self.current_root_menu, bounds],
                            move |event, window, cx| {
                                let bounds = *bounds.read(cx);

                                current_root_menu.write(cx, Some((bounds, menu.clone())));
                            }
                        ),
                    )
                    .on_hover(using!(
                        [self.menu, self.current_root_menu, bounds],
                        move |event, window, cx| {
                            let bounds = *bounds.read(cx);

                            current_root_menu.update(cx, |current_menu, cx| {
                                if let Some(current_menu) = current_menu {
                                    *current_menu = (bounds, menu.clone());
                                }
                            });
                        }
                    )),
            )
            .on_bounds_paint(using!([bounds], move |canvas_bounds, _, cx| {
                bounds.update(cx, move |this, _cx| {
                    *this = canvas_bounds;
                })
            }))
    }
}

pub trait ElementExt: ParentElement
where
    Self: Sized,
{
    fn on_bounds_prepaint(
        self,
        listener: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.child(
            canvas(listener, |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        )
    }

    fn on_bounds_paint(
        self,
        listener: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, cx| listener(bounds, window, cx),
            )
            .top_0()
            .left_0()
            .absolute()
            .size_full(),
        )
    }
}

impl<T: ParentElement> ElementExt for T {}

#[derive(IntoElement)]
struct MenuPopup {
    id: ElementId,
    menu: OwnedMenu,
    current_root_menu: Entity<Option<OpenedMenuPopup>>,
    menu_bounds: Entity<Vec<Bounds<Pixels>>>,
    style: StyleRefinement,
}

impl Styled for MenuPopup {
    #[doc = " Returns a reference to the style memory of this element."]
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl MenuPopup {
    fn new(
        id: impl Into<ElementId>,
        menu: OwnedMenu,
        current_root_menu: Entity<Option<OpenedMenuPopup>>,
        menu_bounds: Entity<Vec<Bounds<Pixels>>>,
    ) -> Self {
        Self {
            id: id.into(),
            menu,
            current_root_menu,
            menu_bounds,
            style: StyleRefinement::default(),
        }
    }
}

impl RenderOnce for MenuPopup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let current_menu =
            window.use_keyed_state((self.id.clone(), "current_menu"), cx, |_, _| None);

        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.darker_background();

        let hover_background = theme.palette.background();

        let foreground = theme.palette.foreground();

        let border = theme.palette.gray();

        drop(theme);

        let viewport_size = window.viewport_size();

        current_menu.update(
            cx,
            using!([self.menu], move |current_menu, cx| {
                if let Some((_, current_menu)) = current_menu
                    && let Some(menu) = menu.items.into_iter().find_map(|menu| match menu {
                        OwnedMenuItem::Submenu(owned_menu) => Some(owned_menu),
                        _ => None,
                    })
                {
                    *current_menu = menu;
                }
            }),
        );

        div()
            // .absolute()
            .occlude()
            .bg(background)
            .border_color(border)
            .border_1()
            // .min_w(px(100.0))
            .flex_basis(px(0.0))
            .flex_col()
            .children(self.menu.items.iter().map(|item| {
                div().child(match item {
                    OwnedMenuItem::Separator => PopupMenuItem::separator(),
                    OwnedMenuItem::Submenu(OwnedMenu { name, .. }) => PopupMenuItem::submenu(
                        item.clone(),
                        (self.id.clone(), name.clone()),
                        self.current_root_menu.clone(),
                        current_menu.clone(),
                    ),
                    OwnedMenuItem::SystemMenu(_) => todo!(),
                    OwnedMenuItem::Action { name, .. } => PopupMenuItem::action(
                        item.clone(),
                        (self.id.clone(), name.clone()),
                        self.current_root_menu.clone(),
                        current_menu.clone(),
                    ),
                })
            }))
            .on_bounds_paint(using!([self.menu_bounds], move |bounds, window, cx| {
                menu_bounds.as_mut(cx).push(bounds);
            }))
            .when_some(
                current_menu.read(cx).clone(),
                using!(
                    [self.id, self.current_root_menu, self.menu_bounds],
                    move |this, (bounds, menu)| {
                        this.child(deferred(
                            anchored()
                                .position_mode(AnchoredPositionMode::Window)
                                .snap_to_window_with_margin(px(16.0))
                                .anchor(Anchor::TopLeft)
                                .position(bounds.top_right())
                                .child(
                                    MenuPopup::new(
                                        (id, menu.name.clone().as_ref()),
                                        menu,
                                        current_root_menu,
                                        menu_bounds,
                                    )
                                    .max_w(viewport_size.width * 0.8),
                                ),
                        ))
                    }
                ),
            )
            .tap_mut(|this| this.style().refine(&self.style))
    }
}

#[derive(IntoElement)]
struct PopupMenuItem {
    menu_item: OwnedMenuItem,
    current_root_menu: Option<Entity<Option<OpenedMenuPopup>>>,
    current_menu: Option<Entity<Option<OpenedMenuPopup>>>,
    id: Option<ElementId>,
}

impl PopupMenuItem {
    fn separator() -> Self {
        PopupMenuItem {
            menu_item: OwnedMenuItem::Separator,
            current_root_menu: None,
            current_menu: None,
            id: None,
        }
    }

    fn action(
        menu_item: OwnedMenuItem,
        id: impl Into<ElementId>,
        current_root_menu: Entity<Option<OpenedMenuPopup>>,
        current_menu: Entity<Option<OpenedMenuPopup>>,
    ) -> Self {
        PopupMenuItem {
            menu_item,
            current_root_menu: Some(current_root_menu),
            current_menu: Some(current_menu),
            id: Some(id.into()),
        }
    }

    fn submenu(
        menu_item: OwnedMenuItem,
        id: impl Into<ElementId>,
        current_root_menu: Entity<Option<OpenedMenuPopup>>,
        current_menu: Entity<Option<OpenedMenuPopup>>,
    ) -> Self {
        PopupMenuItem {
            menu_item,
            current_root_menu: Some(current_root_menu),
            current_menu: Some(current_menu),
            id: Some(id.into()),
        }
    }
}

impl RenderOnce for PopupMenuItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();

        let background = theme.palette.darker_background();

        let hover_background = theme.palette.background();

        let foreground = theme.palette.foreground();
        let darker_foreground = theme.palette.dark_foreground();
        let darkest_foreground = theme.palette.gray();

        let border = theme.palette.gray();

        drop(theme);

        match self.menu_item {
            OwnedMenuItem::Separator => div()
                .h_0()
                .border(px(1.0))
                .border_color(border)
                .flex_grow()
                .flex_shrink()
                .into_any_element(),
            OwnedMenuItem::Submenu(owned_menu) => {
                let OwnedMenu {
                    name,
                    items,
                    disabled,
                } = owned_menu.clone();

                let button_bounds = window.use_keyed_state(
                    (self.id.clone().unwrap(), "submenu_bounds"),
                    cx,
                    |window, cx| Bounds::<Pixels>::default(),
                );

                Button::new(
                    Duration::from_millis(100),
                    (self.id.clone().unwrap(), name.as_str()),
                )
                .on_hover(using!(
                    [self.current_menu, button_bounds],
                    move |&hover_status, window, cx| {
                        if let Some(current_menu) = &current_menu
                            && hover_status
                        {
                            current_menu
                                .write(cx, Some((*button_bounds.read(cx), owned_menu.clone())));
                        };
                    }
                ))
                .flex_grow()
                .flex_shrink()
                .flex()
                .items_center()
                .justify_between()
                .pl_6()
                .pr_1()
                .text_align(TextAlign::Left)
                .child(name)
                .child(
                    svg()
                        .path("icons/menu-right")
                        .size_6()
                        .text_color(foreground),
                )
                .on_bounds_prepaint(using!([button_bounds], move |bounds, window, cx| {
                    button_bounds.write(cx, bounds);
                }))
                .into_any_element()
            }
            OwnedMenuItem::SystemMenu(owned_os_menu) => div()
                .child(owned_os_menu.name)
                .flex_grow()
                .flex_shrink()
                .into_any_element(),
            OwnedMenuItem::Action {
                name,
                action,
                checked,
                disabled,
                ..
            } => {
                let current_root_menu = self.current_root_menu.unwrap().clone();

                let key_bind = window
                    .highest_precedence_binding_for_action(action.as_ref())
                    .map(|binding| {
                        binding
                            .keystrokes()
                            .iter()
                            .map(ToString::to_string)
                            .map(|keystroke| keystroke.to_case(convert_case::Case::Train))
                            .collect::<Vec<_>>()
                            .join(" ")
                    });

                Button::new(
                    Duration::from_millis(100),
                    (self.id.clone().unwrap(), name.as_str()),
                )
                .when_else(
                    disabled,
                    |this| this.disabled(),
                    |this| {
                        this.on_mouse_down(
                            MouseButton::Left,
                            using!([], move |event, window, cx| {
                                let name = action.name();

                                window.dispatch_action(action.boxed_clone(), cx);

                                if !name.to_lowercase().contains("toggle") {
                                    current_root_menu.update(cx, |this, cx| {
                                        *this = None;
                                    })
                                }
                            }),
                        )
                    },
                )
                .flex_grow()
                .flex_shrink()
                .flex()
                .justify_between()
                .items_baseline()
                .pl_6()
                .pr_6()
                .when(checked, |this| {
                    this.child(
                        div()
                            .child(
                                svg()
                                    .path("icons/check")
                                    .size_4()
                                    .aspect_square()
                                    .text_color(if disabled {
                                        darker_foreground
                                    } else {
                                        foreground
                                    }),
                            )
                            .absolute()
                            .left_1()
                            .mt_auto()
                            .mb_auto(),
                    )
                })
                .child(
                    div()
                        .child(SharedString::from(&name))
                        .text_align(TextAlign::Left),
                )
                .when_some(key_bind, |this, keybind| {
                    this.pr_1().child(
                        div()
                            .ml_2p5()
                            .child(format!("({})", keybind))
                            .text_align(TextAlign::Right)
                            .text_sm()
                            .text_color(if disabled {
                                darkest_foreground
                            } else {
                                darker_foreground
                            }),
                    )
                })
                .overflow_x_hidden()
                .on_hover(using!(
                    [self.current_menu],
                    move |&hover_status, window, cx| {
                        if let Some(current_menu) = &current_menu
                            && hover_status
                        {
                            current_menu.write(cx, None);
                        };
                    }
                ))
                .text_color(if disabled {
                    darker_foreground
                } else {
                    foreground
                })
                .into_any_element()
            }
        }
    }
}
