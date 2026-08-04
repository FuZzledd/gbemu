pub mod button;
pub mod menubar;
pub mod root;
pub mod scrollbar;
pub mod titlebar;

pub struct DefaultValue;

pub trait DefaultableInto<T, M = ()> {
    fn defaultable_into(self) -> Option<T>;
}

impl<T: From<U>, U> DefaultableInto<T> for U {
    fn defaultable_into(self) -> Option<T> {
        Some(T::from(self))
    }
}

impl<T> DefaultableInto<T, DefaultValue> for DefaultValue {
    fn defaultable_into(self) -> Option<T> {
        None
    }
}

impl<T> DefaultableInto<T, DefaultValue> for () {
    fn defaultable_into(self) -> Option<T> {
        None
    }
}
