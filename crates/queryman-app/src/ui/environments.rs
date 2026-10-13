//! Окно редактирования окружений и глобальных переменных (FR-40).

use super::{components, kv};
use crate::state::AppState;
use eframe::egui;
use queryman_core::model::Environment;

pub fn show(ctx: &egui::Context, state: &mut AppState) -> bool {
    if !state.show_env_editor {
        return false;
    }
    let mut open = true;
    let mut changed = false;
    let mut select: Option<Option<usize>> = None;
    let mut add = false;
    let mut delete = None;

    egui::Window::new("Окружения")
        .open(&mut open)
        .default_size([720.0, 440.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    if ui.selectable_label(state.env_editor_selection.is_none(), "Глобальные").clicked() {
                        select = Some(None);
                    }
                    ui.separator();
                    for (i, env) in state.environments.iter().enumerate() {
                        let active = state.active_env == Some(env.id);
                        let label = if active { format!("● {}", env.name) } else { env.name.clone() };
                        if ui.selectable_label(state.env_editor_selection == Some(i), label).clicked() {
                            select = Some(Some(i));
                        }
                    }
                    ui.add_space(8.0);
                    if components::outline(ui, "+ Окружение").clicked() {
                        add = true;
                    }
                });
                ui.separator();
                ui.vertical(|ui| match state.env_editor_selection {
                    None => {
                        components::heading(ui, "Глобальные переменные");
                        components::muted(ui, "Действуют всегда; окружение переопределяет их значения.");
                        ui.add_space(6.0);
                        changed |= kv::table(ui, "globals", &mut state.globals, "Переменная", "Значение");
                    }
                    Some(i) => {
                        if let Some(env) = state.environments.get_mut(i) {
                            ui.horizontal(|ui| {
                                ui.label("Название");
                                changed |= ui.text_edit_singleline(&mut env.name).changed();
                                if components::ghost(ui, "Удалить").clicked() {
                                    delete = Some(i);
                                }
                            });
                            ui.add_space(6.0);
                            changed |= kv::table(ui, "env_vars", &mut env.variables, "Переменная", "Значение");
                        }
                    }
                });
            });
        });

    if let Some(selection) = select {
        state.env_editor_selection = selection;
    }
    if add {
        state.environments.push(Environment::default());
        state.env_editor_selection = Some(state.environments.len() - 1);
        changed = true;
    }
    if let Some(i) = delete {
        let removed = state.environments.remove(i);
        if state.active_env == Some(removed.id) {
            state.active_env = None;
        }
        state.env_editor_selection = None;
        changed = true;
    }
    if !open {
        state.show_env_editor = false;
    }
    changed
}
