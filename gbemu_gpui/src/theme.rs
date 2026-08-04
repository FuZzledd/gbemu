extern crate alloc;
use better_default::Default;
use core::cell::RefCell;
use core::cell::{Ref, RefMut};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    sync::{Arc, LazyLock},
};

use gpui::Global;

use gbemu_common::theme::*;

pub static DEFAULT_THEMES: LazyLock<BTreeMap<Cow<'static, str>, Theme>> =
    LazyLock::new(|| include!(concat!(env!("OUT_DIR"), "/themes_generated")));

#[derive(Default, Debug, Clone)]
pub struct ThemeRegistry {
    inner: Arc<RefCell<ThemeRegistryInner>>,
}

impl ThemeRegistry {
    pub fn themes(&self) -> Ref<'_, BTreeMap<Cow<'static, str>, Theme>> {
        Ref::map(self.inner.borrow(), |registry| &registry.themes)
    }

    pub fn themes_mut(&self) -> RefMut<'_, BTreeMap<Cow<'static, str>, Theme>> {
        RefMut::map(self.inner.borrow_mut(), |registry| &mut registry.themes)
    }

    pub fn current_theme_key(&self) -> Cow<'static, str> {
        self.inner.borrow().current_theme.clone()
    }

    pub fn current_theme(&self) -> Ref<'_, Theme> {
        Ref::map(self.inner.borrow(), |registry| {
            &registry.themes[&registry.current_theme]
        })
    }

    pub fn current_theme_mut(&self) -> RefMut<'_, Theme> {
        RefMut::map(self.inner.borrow_mut(), |registry| {
            registry.themes.get_mut(&registry.current_theme).unwrap()
        })
    }

    pub fn set_current_theme(&self, theme: Cow<'static, str>) {
        self.inner.borrow_mut().current_theme = theme;
    }
}

impl Global for ThemeRegistry {}

#[derive(Default, Debug)]
pub struct ThemeRegistryInner {
    #[default(DEFAULT_THEMES.clone())]
    themes: BTreeMap<Cow<'static, str>, Theme>,
    #[default("Catppuccin Frappe".into())]
    current_theme: Cow<'static, str>,
}
