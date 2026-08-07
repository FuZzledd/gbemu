use core::time::Duration;
use gpui::{prelude::*, *};
use uzi::using;

use crate::{
    components::{
        button::Button,
        menubar::menu_popup_button::PopupButton,
        menubarbutton::{ElementExt, OpenedMenuPopup},
    },
    theme::ThemeRegistry,
};

pub struct MenuPopupItem {
    pub item: OwnedMenuItem,
    pub button: Entity<Button>,
}

pub struct MenuPopup {
    pub menu: OwnedMenu,
    pub items: Vec<MenuPopupItem>,
    pub hovered_index: Option<usize>,
    pub active_submenu: Option<(usize, Entity<MenuPopup>)>,
    pub current_popup: Entity<Option<OpenedMenuPopup>>,
    pub menu_bounds: Entity<Vec<Bounds<Pixels>>>,
}

impl MenuPopup {
    pub fn new(
        menu: OwnedMenu,
        current_popup: Entity<Option<OpenedMenuPopup>>,
        menu_bounds: Entity<Vec<Bounds<Pixels>>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let entity = cx.entity();

        let items = menu
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let button_id = ElementId::from(format!("menu_popup_item_{}_{}", menu.name, idx));
                let button = Button::new(cx, button_id, Duration::from_millis(50));

                let entity_for_hover = entity.clone();
                let entity_for_click = entity.clone();

                // Configure button structure and event listeners ONCE upon creation
                button.update(cx, |btn, _| match item {
                    OwnedMenuItem::Action { name, .. } => {
                        let name = name.clone();
                        btn.add_child(move |_, _| {
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .w_full()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .child(name.clone())
                        });

                        btn.on_hover(move |hovered, window, cx| {
                            if *hovered {
                                entity_for_hover.update(cx, |this, cx| {
                                    this.set_hovered(Some(idx), window, cx);
                                });
                            }
                        });

                        btn.on_click(move |_event, window, cx| {
                            entity_for_click.update(cx, |this, cx| {
                                this.select_item(idx, window, cx);
                            });
                        });
                    }

                    OwnedMenuItem::Submenu(submenu) => {
                        let label = submenu.name.clone();
                        btn.add_child(move |_, _| {
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .w_full()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .child(label.clone())
                                .child(div().text_xs().child("›"))
                        });

                        btn.on_hover(move |hovered, window, cx| {
                            if *hovered {
                                entity_for_hover.update(cx, |this, cx| {
                                    this.set_hovered(Some(idx), window, cx);
                                });
                            }
                        });
                    }

                    _ => {}
                });

                MenuPopupItem {
                    item: item.clone(),
                    button,
                }
            })
            .collect();

        Self {
            menu,
            items,
            hovered_index: None,
            active_submenu: None,
            current_popup,
            menu_bounds,
        }
    }

    fn set_hovered(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if self.hovered_index == index {
            return;
        }

        self.hovered_index = index;

        if let Some(idx) = index {
            if let Some(OwnedMenuItem::Submenu(submenu)) = self.menu.items.get(idx) {
                let current_popup = self.current_popup.clone();
                let menu_bounds = self.menu_bounds.clone();
                let submenu_menu = submenu.clone();

                let submenu_entity =
                    cx.new(|cx| MenuPopup::new(submenu_menu, current_popup, menu_bounds, cx));
                self.active_submenu = Some((idx, submenu_entity));
            } else {
                self.active_submenu = None;
            }
        } else {
            self.active_submenu = None;
        }

        cx.notify();
    }

    fn select_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self.menu.items.get(index) {
            match item {
                OwnedMenuItem::Action { action, .. } => {
                    let action = action.boxed_clone();
                    // Close popup hierarchy
                    self.current_popup.write(cx, None);
                    // Dispatch action
                    window.dispatch_action(action, cx);
                }
                _ => {}
            }
        }
    }
}

impl Render for MenuPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();
        let background = theme.palette.darker_background();
        let border = theme.palette.gray();
        drop(theme);

        let menu_bounds = self.menu_bounds.clone();
        let entity_id = cx.entity_id();

        div()
            .id(ElementId::from(("menu_popup", entity_id)))
            .flex()
            .flex_col()
            .min_w(px(160.0))
            .p_1()
            .bg(background)
            .border_1()
            .border_color(border)
            .rounded_md()
            .shadow_lg()
            .on_bounds_prepaint(using!([menu_bounds], move |bounds, _window, cx| {
                menu_bounds.update(cx, |bounds_vec, _| {
                    bounds_vec.push(bounds);
                });
            }))
            .on_hover(cx.listener(|this, hovered, window, cx| {
                eprintln!("MenuPopup hover event: {}", hovered);
                if !hovered {
                    this.set_hovered(None, window, cx);
                }
            }))
            .children(self.items.iter().enumerate().map(|(idx, popup_item)| {
                let menu_id = ElementId::from(("menu", entity_id));
                match &popup_item.item {
                    OwnedMenuItem::Separator => {
                        div().h(px(1.0)).my_1().bg(border).into_any_element()
                    }

                    OwnedMenuItem::Action { .. } => PopupButton::with_button(
                        popup_item.button.clone(),
                        ElementId::from((menu_id.clone(), idx.to_string())),
                    )
                    .into_any_element(),

                    OwnedMenuItem::Submenu(_) => {
                        let btn = PopupButton::with_button(
                            popup_item.button.clone(),
                            ElementId::from((menu_id.clone(), idx.to_string())),
                        );

                        div()
                            .relative()
                            .w_full()
                            .child(btn)
                            .when_some(
                                self.active_submenu
                                    .as_ref()
                                    .filter(|(s_idx, _)| *s_idx == idx),
                                |this, (_, submenu_entity)| {
                                    this.child(deferred(
                                        anchored()
                                            .anchor(Anchor::TopRight)
                                            .child(submenu_entity.clone()),
                                    ))
                                },
                            )
                            .into_any_element()
                    }

                    _ => div().into_any_element(),
                }
            }))
    }
}
