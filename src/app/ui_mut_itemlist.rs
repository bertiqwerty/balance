use egui::Ui;
use serde::{Deserialize, Serialize};
use std::mem;

use crate::{container_util::remove_indices, core_types::BlcResult};

#[derive(Default, Deserialize, Serialize)]
pub struct MutItemList<T: Default> {
    items: Vec<T>,
}
impl<T: Default> MutItemList<T> {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        mut show_item: impl FnMut(usize, &mut T, &mut Ui),
        mut make_item: impl FnMut() -> BlcResult<T>,
        add_label: &str
    ) {
        if ui.button(add_label).clicked() {
            if let Ok(item) = make_item() {
                self.items.push(item)
            }
        }
        ui.end_row();
        let mut to_be_deleted = vec![];
        for (i, item) in self.items.iter_mut().enumerate() {
            show_item(i, item, ui);
            if ui.button("x").clicked() {
                to_be_deleted.push(i);
            }
            ui.end_row();
        }
        if !self.items.is_empty() {
            self.items = remove_indices(mem::take(&mut self.items), &to_be_deleted);
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}
