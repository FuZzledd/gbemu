use gpui::{prelude::*, *};
use uzi::using;

use crate::{
    components::menubarbutton::{ElementExt, MenuBarButton},
    theme::ThemeRegistry,
};

use super::menu_popup::MenuPopup;

pub struct MenuBar {
    bounds: Bounds<Pixels>,
    popup_entity: Option<Entity<MenuPopup>>,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            bounds: Default::default(),
            popup_entity: None,
        }
    }
}

impl Default for MenuBar {
    fn default() -> Self {
        Self::new()
    }
}

actions!(menubar, [CloseMenus]);

impl Render for MenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme().clone();

        let background = theme.palette.darker_background();
        let _hover_background = theme.palette.background();
        let foreground = theme.palette.foreground();
        let border = theme.palette.gray();

        let app_menu = window.use_state(cx, |_, cx| cx.get_menus().unwrap_or_default());

        let entity_id = cx.entity_id();
        let element_id = ElementId::from(("menubar", entity_id));

        let current_popup: Entity<Option<(Bounds<Pixels>, OwnedMenu)>> =
            window.use_state(cx, |_window, _cx| None);

        let entity = cx.entity();

        let menu_bounds: Entity<Vec<Bounds<Pixels>>> =
            window.use_keyed_state((element_id.clone(), "total_bounds"), cx, |_, _| vec![]);

        let menu_buttons =
            window.use_keyed_state((element_id.clone(), "menu_buttons"), cx, |_, cx| {
                let menus = cx.get_menus().unwrap_or_default();
                app_menu.write(cx, menus.clone());
                menus
                    .into_iter()
                    .map(|menu| {
                        let id = ElementId::from((element_id.clone(), menu.name.clone()));
                        MenuBarButton::new(cx, id, menu, current_popup.clone())
                    })
                    .collect::<Vec<_>>()
            });

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

        let menus = menu_buttons.read(cx).clone();

        cx.subscribe_self(using!(
            [current_popup],
            move |_this, _: &DismissEvent, cx| {
                println!("[DEBUG] MenuBar received DismissEvent, clearing current_popup");
                current_popup.write(cx, None);
            }
        ))
        .detach();

        let active_menu = current_popup.read(cx).clone();
        let popup_info = if let Some((bounds, ref menu)) = active_menu {
            let mut recreate = true;
            if let Some(ref existing_popup) = self.popup_entity {
                if existing_popup.read(cx).menu.name == menu.name {
                    recreate = false;
                }
            }
            let popup_view = if recreate {
                let current_popup = current_popup.clone();
                let menu_bounds = menu_bounds.clone();
                let menu = menu.clone();
                let new_popup = cx.new(|cx| MenuPopup::new(menu, current_popup, menu_bounds, cx));
                self.popup_entity = Some(new_popup.clone());
                new_popup
            } else {
                self.popup_entity.as_ref().unwrap().clone()
            };
            Some((bounds, popup_view))
        } else {
            self.popup_entity = None;
            None
        };

        div()
            .on_mouse_down_out(using!([menu_bounds, entity], move |event, _window, cx| {
                let mouse_pos = event.position;
                println!(
                    "[DEBUG] MenuBar on_mouse_down_out at position: {:?}",
                    mouse_pos
                );

                if menu_bounds
                    .read(cx)
                    .iter()
                    .all(|bounds| !bounds.contains(&mouse_pos))
                {
                    println!("[DEBUG] Click outside menu bounds detected, emitting DismissEvent");
                    entity.update(cx, |_this, cx| {
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
            .when_some(popup_info, move |this, (bounds, popup_view)| {
                this.child(deferred(
                    anchored()
                        .position_mode(AnchoredPositionMode::Window)
                        .snap_to_window_with_margin(px(16.0))
                        .anchor(Anchor::TopLeft)
                        .position(bounds.bottom_left())
                        .child(popup_view),
                ))
            })
            .on_bounds_prepaint(using!([entity, menu_bounds], move |bounds, _, cx| {
                entity.update(cx, |this, cx| {
                    this.bounds = bounds;
                    menu_bounds.as_mut(cx).clear();
                })
            }))
    }
}

impl<T: 'static> EventEmitter<T> for MenuBar {}
