use gpui::*;

pub trait ElementBoundsExt: ParentElement
where
    Self: Sized,
{
    fn on_bounds_prepaint(
        self,
        listener: impl FnOnce(&Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.child(
            canvas(
                |bounds, window, cx| listener(&bounds, window, cx),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }

    fn on_bounds_paint(
        self,
        listener: impl FnOnce(&Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, cx| listener(&bounds, window, cx),
            )
            .top_0()
            .left_0()
            .absolute()
            .size_full(),
        )
    }
}

impl<T: ParentElement> ElementBoundsExt for T {}

pub trait EntityStyleExt {
    fn update_style(&self, cx: &mut App, func: impl Fn(&mut StyleRefinement, &mut App));
}

impl<T: Styled + 'static> EntityStyleExt for Entity<T> {
    fn update_style(&self, cx: &mut App, func: impl Fn(&mut StyleRefinement, &mut App)) {
        self.update(cx, |entity, cx| func(entity.style(), cx))
    }
}
