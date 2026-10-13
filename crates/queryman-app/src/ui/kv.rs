//! Таблица ключ-значение: Params, Headers, x-www-form-urlencoded, переменные.
//! В конце всегда есть пустая строка: ввод в неё добавляет новую.

use super::components;
use eframe::egui;
use queryman_core::model::{KeyValue, Variable};

pub trait KvRow: Default {
    fn key_mut(&mut self) -> &mut String;
    fn value_mut(&mut self) -> &mut String;
    fn enabled_mut(&mut self) -> &mut bool;
    fn is_blank(&self) -> bool;
    fn is_secret(&self) -> bool {
        false
    }
}

impl KvRow for KeyValue {
    fn key_mut(&mut self) -> &mut String {
        &mut self.key
    }
    fn value_mut(&mut self) -> &mut String {
        &mut self.value
    }
    fn enabled_mut(&mut self) -> &mut bool {
        &mut self.enabled
    }
    fn is_blank(&self) -> bool {
        KeyValue::is_blank(self)
    }
}

impl KvRow for Variable {
    fn key_mut(&mut self) -> &mut String {
        &mut self.key
    }
    fn value_mut(&mut self) -> &mut String {
        &mut self.value
    }
    fn enabled_mut(&mut self) -> &mut bool {
        &mut self.enabled
    }
    fn is_blank(&self) -> bool {
        Variable::is_blank(self)
    }
    fn is_secret(&self) -> bool {
        self.secret
    }
}

/// Возвращает `true`, если содержимое изменилось.
pub fn table<R: KvRow>(ui: &mut egui::Ui, id: &str, rows: &mut Vec<R>, key_hint: &str, value_hint: &str) -> bool {
    if rows.last().is_none_or(|r| !r.is_blank()) {
        rows.push(R::default());
    }
    let mut changed = false;
    let mut remove = None;
    let count = rows.len();
    let column = ((ui.available_width() - 90.0) / 2.0).max(140.0);

    egui::Grid::new(id).num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        for (i, row) in rows.iter_mut().enumerate() {
            let is_last = i + 1 == count;
            changed |= components::checkbox(ui, row.enabled_mut()).changed();
            changed |= components::input(ui, row.key_mut(), key_hint, column).changed();
            let secret = row.is_secret();
            let value = if secret {
                components::secret_input(ui, row.value_mut(), value_hint, column)
            } else {
                components::input(ui, row.value_mut(), value_hint, column)
            };
            changed |= value.changed();
            if is_last {
                ui.label("");
            } else if components::ghost(ui, "×").on_hover_text("Удалить строку").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });

    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    changed
}
