//! Боковая панель: дерево коллекций (FR-30) и история запросов (FR-33).

use super::components;
use crate::state::{AppState, Dialog, Node};
use eframe::egui::{self, RichText};
use queryman_core::collection::{Folder, HistoryEntry, Item};
use queryman_core::model::Request;
use uuid::Uuid;

/// Действие, выбранное в панели; применяется после отрисовки, когда заимствования сняты.
enum Action {
    Open(Uuid),
    OpenHistory(usize),
    NewCollection,
    NewFolder(Node),
    NewRequest(Node),
    Rename(Node),
    Duplicate(Node),
    EditAuth(Node),
    Delete(Node),
    DeleteHistory(usize),
    ClearHistory,
}

pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> bool {
    let mut actions = Vec::new();
    egui::Panel::left("sidebar").resizable(true).default_size(260.0).size_range(200.0..=460.0).show(ui, |ui| {
        ui.add_space(8.0);
        let history_label = format!("История ({})", state.history.len());
        components::tabs(ui, &mut state.sidebar_section, &["Коллекции", history_label.as_str()]);
        ui.add_space(6.0);
        if state.sidebar_section == 0 {
            collections(ui, state, &mut actions);
        } else {
            history(ui, &state.history, &mut actions);
        }
    });

    let mut changed = false;
    for action in actions {
        changed |= apply(state, action);
    }
    changed
}

fn collections(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        components::heading(ui, "Коллекции");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if components::ghost(ui, "+").on_hover_text("Новая коллекция").clicked() {
                actions.push(Action::NewCollection);
            }
        });
    });
    if state.collections.is_empty() {
        components::muted(ui, "Коллекций пока нет. Создайте коллекцию или сохраните запрос (Ctrl+S).");
        ui.add_space(6.0);
        if components::outline(ui, "Создать коллекцию").clicked() {
            actions.push(Action::NewCollection);
        }
        return;
    }

    let AppState { collections, tree_selection, .. } = state;
    egui::ScrollArea::vertical().id_salt("collections_scroll").auto_shrink([false, false]).show(ui, |ui| {
        components::tree(ui, "collections", tree_selection, |tree| {
            for c in collections.iter() {
                let node = Node::Collection(c.id);
                let label = format!("{}\u{1f}{}", c.name, c.id);
                let response = tree.folder_ui(
                    node,
                    &label,
                    |ui| {
                        ui.label(RichText::new(&c.name).strong());
                    },
                    |tree| items(tree, &c.items, actions),
                );
                response.context_menu(|ui| container_menu(ui, node, actions));
            }
        });
    });
}

fn items(tree: &mut components::TreeUi<'_, Node>, list: &[Item], actions: &mut Vec<Action>) {
    for item in list {
        match item {
            Item::Folder(f) => folder(tree, f, actions),
            Item::Request(r) => request(tree, r, actions),
        }
    }
}

fn folder(tree: &mut components::TreeUi<'_, Node>, f: &Folder, actions: &mut Vec<Action>) {
    let node = Node::Folder(f.id);
    let label = format!("{}\u{1f}{}", f.name, f.id);
    let response = tree.folder_ui(
        node,
        &label,
        |ui| {
            ui.label(&f.name);
        },
        |tree| items(tree, &f.items, actions),
    );
    response.context_menu(|ui| container_menu(ui, node, actions));
}

fn request(tree: &mut components::TreeUi<'_, Node>, r: &Request, actions: &mut Vec<Action>) {
    let node = Node::Request(r.id);
    let label = format!("{}\u{1f}{}", r.name, r.id);
    let response = tree.leaf_ui(node, &label, |ui| {
        ui.label(components::method_text(&r.method).size(11.0));
        ui.add(egui::Label::new(&r.name).truncate());
    });
    if response.clicked() {
        actions.push(Action::Open(r.id));
    }
    response.context_menu(|ui| {
        if ui.button("Открыть").clicked() {
            actions.push(Action::Open(r.id));
        }
        if ui.button("Переименовать").clicked() {
            actions.push(Action::Rename(node));
        }
        if ui.button("Дублировать").clicked() {
            actions.push(Action::Duplicate(node));
        }
        ui.separator();
        if ui.button("Удалить").clicked() {
            actions.push(Action::Delete(node));
        }
    });
}

fn container_menu(ui: &mut egui::Ui, node: Node, actions: &mut Vec<Action>) {
    if ui.button("Новый запрос").clicked() {
        actions.push(Action::NewRequest(node));
    }
    if ui.button("Новая папка").clicked() {
        actions.push(Action::NewFolder(node));
    }
    ui.separator();
    if ui.button("Переименовать").clicked() {
        actions.push(Action::Rename(node));
    }
    if ui.button("Авторизация…").clicked() {
        actions.push(Action::EditAuth(node));
    }
    if ui.button("Дублировать").clicked() {
        actions.push(Action::Duplicate(node));
    }
    ui.separator();
    if ui.button("Удалить").clicked() {
        actions.push(Action::Delete(node));
    }
}

fn history(ui: &mut egui::Ui, history: &[HistoryEntry], actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        components::heading(ui, "История");
        if !history.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if components::ghost(ui, "Очистить").clicked() {
                    actions.push(Action::ClearHistory);
                }
            });
        }
    });
    if history.is_empty() {
        components::muted(ui, "Здесь появятся отправленные запросы. Тела ответов не сохраняются.");
        return;
    }
    let now = crate::app::unix_ms();
    egui::ScrollArea::vertical().id_salt("history_scroll").auto_shrink([false, false]).show_rows(
        ui,
        44.0,
        history.len(),
        |ui, range| {
            for i in range {
                let entry = &history[i];
                let response = ui
                    .push_id(entry.id, |ui| {
                        ui.set_min_height(40.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(components::method_text(&entry.request.method).size(11.0));
                                match (entry.status, &entry.error) {
                                    (Some(status), _) => {
                                        ui.label(
                                            RichText::new(status.to_string()).color(components::status_color(status)),
                                        );
                                    }
                                    (None, Some(_)) => {
                                        ui.label(RichText::new("ошибка").color(components::ERROR_COLOR));
                                    }
                                    (None, None) => {}
                                }
                                if let Some(ms) = entry.elapsed_ms {
                                    components::muted(ui, &format!("{ms} мс"));
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    components::muted(ui, &time_ago(now, entry.sent_at_ms));
                                });
                            });
                            let url = if entry.request.url.is_empty() { "(без URL)" } else { &entry.request.url };
                            ui.add(egui::Label::new(RichText::new(url).small()).truncate());
                        });
                    })
                    .response;
                let response =
                    ui.interact(response.rect, ui.id().with(("history_row", entry.id)), egui::Sense::click());
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let response = match &entry.error {
                    Some(error) => response.on_hover_text(error.as_str()),
                    None => response.on_hover_text("Открыть в новой вкладке"),
                };
                if response.clicked() {
                    actions.push(Action::OpenHistory(i));
                }
                response.context_menu(|ui| {
                    if ui.button("Открыть").clicked() {
                        actions.push(Action::OpenHistory(i));
                    }
                    if ui.button("Удалить запись").clicked() {
                        actions.push(Action::DeleteHistory(i));
                    }
                });
            }
        },
    );
}

fn time_ago(now_ms: u64, then_ms: u64) -> String {
    let secs = now_ms.saturating_sub(then_ms) / 1000;
    match secs {
        0..=59 => "только что".to_owned(),
        60..=3599 => format!("{} мин назад", secs / 60),
        3600..=86_399 => format!("{} ч назад", secs / 3600),
        _ => format!("{} дн назад", secs / 86_400),
    }
}

/// Возвращает `true`, если данные изменились.
fn apply(state: &mut AppState, action: Action) -> bool {
    match action {
        Action::Open(id) => {
            state.open_saved(id);
            true
        }
        Action::OpenHistory(i) => {
            if let Some(entry) = state.history.get(i) {
                // Новый id: запрос из истории не связан с коллекцией.
                let request = Request { id: Uuid::new_v4(), ..entry.request.clone() };
                state.open_tab(request);
            }
            true
        }
        Action::NewCollection => {
            state.dialog = Some(Dialog::Create { parent: None, name: "Новая коллекция".to_owned() });
            false
        }
        Action::NewFolder(parent) => {
            state.dialog = Some(Dialog::Create { parent: Some(parent), name: "Новая папка".to_owned() });
            false
        }
        Action::NewRequest(parent) => {
            state.add_child(parent, Item::Request(Request::default()));
            true
        }
        Action::Rename(node) => {
            state.dialog = Some(Dialog::Rename { node, name: state.node_name(node).unwrap_or_default() });
            false
        }
        Action::Duplicate(node) => {
            state.duplicate(node);
            true
        }
        Action::EditAuth(node) => {
            if let Some(auth) = state.node_auth(node) {
                state.dialog = Some(Dialog::EditAuth { node, auth });
            }
            false
        }
        Action::Delete(node) => {
            state.dialog = Some(Dialog::Delete { node, title: state.node_name(node).unwrap_or_default() });
            false
        }
        Action::DeleteHistory(i) => {
            if i < state.history.len() {
                state.history.remove(i);
            }
            true
        }
        Action::ClearHistory => {
            state.dialog = Some(Dialog::ClearHistory);
            false
        }
    }
}
