use core::{cell::RefCell, ops::IndexMut};

use indexmap::IndexMap;

use slint::{Keys, Model, ModelNotify, SharedString, ToSharedString};
use slint_generated::ShortcutEntry;

pub struct ShortcutsModel {
    shortcuts: RefCell<IndexMap<String, ShortcutEntry>>,
    notify: ModelNotify,
}

impl ShortcutsModel {
    pub fn insert(&self, key: String, value: ShortcutEntry) {
        let (index, old_value) = self.shortcuts.borrow_mut().insert_full(key, value);
        if old_value.is_some() {
            self.notify.row_changed(index);
        } else {
            self.notify.row_added(index, 1);
        }
    }

    pub fn get(&self, key: &String) -> Option<i32> {
        self.shortcuts
            .borrow()
            .get_index_of(key)
            .and_then(|x| x.try_into().ok())
    }
}

impl From<IndexMap<String, ShortcutEntry>> for ShortcutsModel {
    fn from(value: IndexMap<String, ShortcutEntry>) -> Self {
        Self {
            notify: ModelNotify::default(),
            shortcuts: RefCell::new(value),
        }
    }
}

impl Model for ShortcutsModel {
    type Data = (ShortcutEntry, SharedString);

    fn row_count(&self) -> usize {
        self.shortcuts.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.shortcuts
            .borrow()
            .get_index(row)
            .map(|(key, entry)| (entry.clone(), key.to_shared_string()))
    }

    fn set_row_data(&self, row: usize, data: Self::Data) {
        *self.shortcuts.borrow_mut().index_mut(row) = data.0;
        // don't forget to call row_changed
        self.notify.row_changed(row);
    }

    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
