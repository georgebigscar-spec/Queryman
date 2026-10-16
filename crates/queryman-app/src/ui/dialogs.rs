//! Модальные диалоги коллекций и истории: переименование, удаление,
//! авторизация папки, сохранение запроса, очистка истории, импорт.

use super::{components, request};
use crate::state::{AppState, Dialog, Node};
use eframe::egui;
use queryman_core::collection::{Folder, Item};
use queryman_core::protocol::Command;

#[derive(Default)]
pub struct DialogOutcome {
    /// Данные изменились и их нужно сохранить.
    pub changed: bool,
    /// Текст, который пользователь попросил импортировать.
    pub import: Option<String>,
}

pub fn show(ctx: &egui::Context, state: &mut AppState) -> DialogOutcome {
    let Some(mut dialog) = state.dialog.take() else { return DialogOutcome::default() };
    let mut changed = false;
    let mut import = None;

    let outcome = match &mut dialog {
        Dialog::Create { parent, name } => {
            let title = if parent.is_some() { "Новая папка" } else { "Новая коллекция" };
            name_dialog(ctx, "create", title, "Создать", name)
        }
        Dialog::Rename { name, .. } => name_dialog(ctx, "rename", "Переименовать", "Сохранить", name),
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
                    // Последний пункт — «Новая коллекция» (`destination == None`).
                    let mut options: Vec<&str> = destinations.iter().map(|d| d.label.as_str()).collect();
                    options.push("Новая коллекция");
                    let mut selected = Some(destination.unwrap_or(destinations.len()));
                    if components::combobox(ui, &mut selected, &options, "Поиск коллекции или папки…", width)
                    {
                        *destination = selected.filter(|&i| i < destinations.len());
                    }
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
        Dialog::Import { text } => {
            let (answer, close) = components::dialog(
                ctx,
                "import",
                "Импорт",
                Some("Вставьте команду cURL, коллекцию или окружение Postman (JSON) либо выберите файл."),
                |ui| {
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(text)
                                .code_editor()
                                .hint_text("curl https://api.example.com/users -H 'Accept: application/json'")
                                .desired_width(f32::INFINITY)
                                .desired_rows(12),
                        );
                    });
                    ui.add_space(6.0);
                    if components::outline(ui, "Выбрать файл…").clicked() {
                        state.outbox.push(Command::PickImportFile);
                    }
                    components::dialog_footer(ui, "Импортировать", false)
                },
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
                Dialog::Create { parent, name } => {
                    let name = name.trim().to_owned();
                    match parent {
                        None => {
                            let name = if name.is_empty() { "Новая коллекция".to_owned() } else { name };
                            let id = state.add_collection(name);
                            state.tree_selection = Some(Node::Collection(id));
                        }
                        Some(parent) => {
                            let name = if name.is_empty() { "Новая папка".to_owned() } else { name };
                            state.add_child(parent, Item::Folder(Folder { name, ..Default::default() }));
                        }
                    }
                }
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
                Dialog::Import { text } => {
                    // Диалог закроет приложение после успешного импорта, при ошибке он остаётся открытым.
                    changed = false;
                    import = Some(text.clone());
                    state.dialog = Some(Dialog::Import { text });
                }
            }
        }
    }
    DialogOutcome { changed, import }
}

/// Диалог с одним полем названия: Enter подтверждает, Esc и «Отмена» закрывают.
fn name_dialog(ctx: &egui::Context, id: &str, title: &str, action: &str, name: &mut String) -> Option<bool> {
    let (answer, close) = components::dialog(ctx, id, title, None, |ui| {
        let input = components::input(ui, name, "Название", ui.available_width());
        if ui.memory(|m| m.focused().is_none()) {
            input.request_focus();
        }
        let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        components::dialog_footer(ui, action, false).or(enter.then_some(true))
    });
    answer.or(close.then_some(false))
}
