use egui::Ui;
use serde::{Deserialize, Serialize};

use crate::core_types::BlcResult;

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
        add_label: &str,
    ) -> Option<(usize, T)> {
        if ui.button(add_label).clicked() {
            if let Ok(item) = make_item() {
                self.items.push(item)
            }
        }
        ui.end_row();
        let mut to_be_deleted = None;
        for (i, item) in self.items.iter_mut().enumerate() {
            show_item(i, item, ui);
            if ui.button("x").clicked() {
                to_be_deleted = Some(i);
            }
            ui.end_row();
        }
        if let Some(to_be_deleted) = to_be_deleted {
            let removed = self.items.remove(to_be_deleted);
            Some((to_be_deleted, removed))
        } else {
            None
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}
