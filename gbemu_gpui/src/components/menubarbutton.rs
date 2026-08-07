use core::time::Duration;
use gpui::{prelude::*, *};

use crate::{components::button::Button, theme::ThemeRegistry};

pub type OpenedMenuPopup = (Bounds<Pixels>, OwnedMenu);

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

pub struct MenuBarButton {
    id: ElementId,
    menu: OwnedMenu,
    current_root_menu: Entity<Option<OpenedMenuPopup>>,
    button: Entity<Button>,
    bounds: Bounds<Pixels>,
}
impl MenuBarButton {
    pub fn new(
        cx: &mut App,
        id: impl Into<ElementId>,
        menu: OwnedMenu,
        current_root_menu: Entity<Option<OpenedMenuPopup>>,
    ) -> Entity<Self> {
        let id = id.into();

        cx.new(|cx| {
            let button = Button::new(cx, id.clone(), Duration::from_millis(200));
            let entity: Entity<Self> = cx.entity();

            let menu_name = menu.name.clone();
            let menu_for_hover = menu.clone();
            let current_root_for_hover = current_root_menu.clone();
            let entity_for_hover = entity.clone();

            let menu_for_click = menu.clone();
            let current_root_for_click = current_root_menu.clone();
            let entity_for_click = entity.clone();

            button.update(cx, |btn, _| {
                btn.add_child(move |_, _| {
                    div()
                        .h(relative(0.8))
                        .p_0p5()
                        .pl_1()
                        .pr_1()
                        .flex()
                        .justify_center()
                        .items_center()
                        .rounded_sm()
                        .child(menu_name.clone())
                });

                btn.on_hover(move |hover_status, _window, cx| {
                    if *hover_status {
                        entity_for_hover.update(cx, |this, cx| {
                            let b = this.bounds;
                            current_root_for_hover.update(cx, |current_menu, _cx| {
                                if let Some(current_menu) = current_menu {
                                    *current_menu = (b, menu_for_hover.clone());
                                }
                            });
                        });
                    }
                });

                btn.on_click(move |_event, _window, cx| {
                    entity_for_click.update(cx, |this, cx| {
                        let b = this.bounds;
                        current_root_for_click.update(cx, |current_menu, _cx| {
                            *current_menu = Some((b, menu_for_click.clone()));
                        });
                    });
                });
            });

            Self {
                id,
                menu,
                current_root_menu,
                button,
                bounds: Bounds::default(),
            }
        })
    }
}
impl Render for MenuBarButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeRegistry>().current_theme();
        let background = theme.palette.darker_background();
        let hover_background = theme.palette.background();
        drop(theme);

        self.button.update(cx, |btn, _| {
            btn.set_background(background)
                .set_hover_background(hover_background);
        });

        let entity = cx.entity();

        div()
            .id((self.id.clone(), "container"))
            .flex()
            .items_center()
            .h_full()
            .child(self.button.clone())
            .on_bounds_paint(move |canvas_bounds, _, cx| {
                entity.update(cx, move |this, _cx| {
                    this.bounds = canvas_bounds;
                });
            })
    }
}
