//! Модальные диалоги коллекций и истории: переименование, удаление,
//! авторизация папки, сохранение запроса, очистка истории.

use super::{components, request};
use crate::state::{AppState, Dialog, Node};
use eframe::egui;

/// Возвращает `true`, если данные изменились и их нужно сохранить.
pub fn show(ctx: &egui::Context, state: &mut AppState) -> bool {
    let Some(mut dialog) = state.dialog.take() else { return false };
    let mut changed = false;

    let outcome = match &mut dialog {
        Dialog::Rename { name, .. } => {
            let (answer, close) = components::dialog(ctx, "rename", "Переименовать", None, |ui| {
                let input = components::input(ui, name, "Название", ui.available_width());
                if ui.memory(|m| m.focused().is_none()) {
                    input.request_focus();
                }
                let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                components::dialog_footer(ui, "Сохранить", false).or(enter.then_some(true))
            });
            answer.or(close.then_some(false))
        }
        Dialog::Delete { node, title } => {
            let what = match node {
                Node::Collection(_) => "Коллекция будет удалена вместе со всеми папками и запросами.",
                Node::Folder(_) => "Папка будет удалена вместе со всем содержимым.",
                Node::Request(_) => "Запрос будет удалён из коллекции. Открытая вкладка останется.",
            };
            let (answer, close) =
                components::dialog(ctx, "delete", &format!("Удалить «{title}»?"), Some(what), |ui| {
                    components::dialog_footer(ui, "Удалить", true)
                });
            answer.or(close.then_some(false))
        }
        Dialog::EditAuth { node, auth } => {
            let hint = match node {
                Node::Collection(_) => "Запросы с авторизацией «Наследовать» получат эти настройки.",
                _ => "«Наследовать» — взять авторизацию родительской папки или коллекции.",
            };
            let (answer, close) = components::dialog(ctx, "edit_auth", "Авторизация", Some(hint), |ui| {
                ui.push_id("dialog_auth", |ui| request::auth_editor(ui, auth));
                components::dialog_footer(ui, "Сохранить", false)
            });
            answer.or(close.then_some(false))
        }
        Dialog::SaveRequest { name, destination, new_collection, .. } => {
            let destinations = state.destinations();
            let (answer, close) =
                components::dialog(ctx, "save_request", "Сохранить запрос", None, |ui| {
                    let width = ui.available_width();
                    components::muted(ui, "Название");
                    components::input(ui, name, "Название запроса", width);
                    ui.add_space(8.0);
                    components::muted(ui, "Куда");
                    let selected = destination
                        .and_then(|i| destinations.get(i))
                        .map_or("Новая коллекция", |d| d.label.as_str())
                        .to_owned();
                    egui::ComboBox::from_id_salt("save_destination").selected_text(selected).width(width).show_ui(
                        ui,
                        |ui| {
                            for (i, d) in destinations.iter().enumerate() {
                                ui.selectable_value(destination, Some(i), d.label.as_str());
                            }
                            ui.selectable_value(destination, None, "Новая коллекция");
                        },
                    );
                    if destination.is_none() {
                        ui.add_space(8.0);
                        components::input(ui, new_collection, "Название коллекции", width);
                    }
                    components::dialog_footer(ui, "Сохранить", false)
                });
            answer.or(close.then_some(false))
        }
        Dialog::ClearHistory => {
            let (answer, close) = components::dialog(
                ctx,
                "clear_history",
                "Очистить историю?",
                Some("Все записи истории будут удалены."),
                |ui| components::dialog_footer(ui, "Очистить", true),
            );
            answer.or(close.then_some(false))
        }
    };

    match outcome {
        None => state.dialog = Some(dialog),
        Some(false) => {}
        Some(true) => {
            changed = true;
            match dialog {
                Dialog::Rename { node, name } => {
                    if !name.trim().is_empty() {
                        state.rename(node, name.trim().to_owned());
                    }
                }
                Dialog::Delete { node, .. } => state.delete(node),
                Dialog::EditAuth { node, auth } => state.set_node_auth(node, auth),
                Dialog::SaveRequest { tab, name, destination, new_collection } => {
                    let destination = destination.and_then(|i| state.destinations().get(i).cloned());
                    let name = if name.trim().is_empty() {
                        "Новый запрос".to_owned()
                    } else {
                        name.trim().to_owned()
                    };
                    state.save_tab_as(tab, name, destination, new_collection.trim().to_owned());
                    components::notify(ctx, "Запрос сохранён");
                }
                Dialog::ClearHistory => state.history.clear(),
            }
        }
    }
    changed
}
