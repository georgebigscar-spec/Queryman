//! Шапка: имя приложения, выбор активного окружения (FR-40).

use super::components;
use crate::state::AppState;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    egui::Panel::top("header").show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(queryman_core::APP_NAME).strong().size(16.0));
            ui.add_space(16.0);
            components::muted(ui, "Ctrl+K — команды · Ctrl+Enter — отправить · Ctrl+T — новая вкладка");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if components::outline(ui, "Окружения").clicked() {
                    state.show_env_editor = true;
                }
                // 0 — «Без окружения», дальше окружения по порядку.
                let mut options = vec!["Без окружения".to_owned()];
                options.extend(state.environments.iter().map(|e| e.name.clone()));
                let before = state.active_env;
                let mut selected = Some(
                    state
                        .active_env
                        .and_then(|id| state.environments.iter().position(|e| e.id == id))
                        .map_or(0, |i| i + 1),
                );
                if components::combobox(ui, &mut selected, &options, "Поиск окружения…", 200.0) {
                    state.active_env = selected.and_then(|i| i.checked_sub(1)).map(|i| state.environments[i].id);
                }
                changed |= state.active_env != before;
            });
        });
        ui.add_space(6.0);
    });
    changed
}
