//! Шапка: имя приложения, выбор активного окружения (FR-40).

use super::components;
use crate::state::AppState;
use eframe::egui;

pub fn show(ctx: &egui::Context, state: &mut AppState) -> bool {
    let mut changed = false;
    egui::TopBottomPanel::top("header").show(ctx, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(queryman_core::APP_NAME).strong().size(16.0));
            ui.add_space(16.0);
            components::muted(ui, "Ctrl+Enter — отправить · Ctrl+T — новая вкладка");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if components::outline(ui, "Окружения").clicked() {
                    state.show_env_editor = true;
                }
                let selected = state.active_environment().map(|e| e.name.clone()).unwrap_or_else(|| "Без окружения".to_owned());
                let before = state.active_env;
                egui::ComboBox::from_id_salt("active_env")
                    .selected_text(selected)
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut state.active_env, None, "Без окружения");
                        for env in &state.environments {
                            ui.selectable_value(&mut state.active_env, Some(env.id), env.name.as_str());
                        }
                    });
                changed |= state.active_env != before;
            });
        });
        ui.add_space(6.0);
    });
    changed
}
