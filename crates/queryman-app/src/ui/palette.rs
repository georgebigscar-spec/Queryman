//! Палитра команд (Ctrl+K): действия приложения и поиск по сохранённым запросам.

use super::components;
use crate::state::{AppState, Dialog};
use uuid::Uuid;

/// Действия, которые выполняет `QuerymanApp` (отправка, сохранение, cURL).
#[derive(Default)]
pub struct PaletteAction {
    pub send: bool,
    pub save: bool,
    pub copy_curl: bool,
    pub changed: bool,
}

enum Pick {
    Send,
    Save,
    CopyCurl,
    NewTab,
    CloseTab,
    SwitchTab(usize),
    OpenSaved(Uuid),
    Environment(Option<Uuid>),
    EditEnvironments,
    NewCollection,
    Import,
    History,
}

pub fn show(ctx: &eframe::egui::Context, state: &mut AppState) -> PaletteAction {
    let mut action = PaletteAction::default();
    if !state.palette_open {
        // Закрытая палитра сбрасывает строку поиска, чтобы открываться пустой.
        components::command_palette(ctx, &mut state.palette_open, |_| ());
        return action;
    }

    let requests: Vec<(String, String, Uuid)> = state
        .collections
        .iter()
        .flat_map(|c| c.requests())
        .map(|(path, r)| (format!("{} {} — {path}", r.method.as_str(), r.name), r.url.clone(), r.id))
        .collect();
    let tabs: Vec<String> =
        state.tabs.iter().map(|t| format!("{} {}", t.request.method.as_str(), t.request.name)).collect();
    let active_env = state.active_env;
    let environments: Vec<(String, Uuid)> =
        state.environments.iter().map(|e| (format!("Окружение: {}", e.name), e.id)).collect();

    let mut picked = None;
    components::command_palette(ctx, &mut state.palette_open, |cmd| {
        cmd.group("Запрос", |cmd| {
            if cmd.item("Отправить", Some("Ctrl+Enter")) {
                picked = Some(Pick::Send);
            }
            if cmd.item("Сохранить в коллекцию", Some("Ctrl+S")) {
                picked = Some(Pick::Save);
            }
            if cmd.item("Скопировать как cURL", None) {
                picked = Some(Pick::CopyCurl);
            }
            if cmd.item("Новая вкладка", Some("Ctrl+T")) {
                picked = Some(Pick::NewTab);
            }
            if cmd.item("Закрыть вкладку", Some("Ctrl+W")) {
                picked = Some(Pick::CloseTab);
            }
        });
        if !requests.is_empty() {
            cmd.group("Сохранённые запросы", |cmd| {
                for (label, url, id) in &requests {
                    if cmd.item_with_keywords(label, url, None) {
                        picked = Some(Pick::OpenSaved(*id));
                    }
                }
            });
        }
        if tabs.len() > 1 {
            cmd.group("Открытые вкладки", |cmd| {
                for (i, label) in tabs.iter().enumerate() {
                    if cmd.item_with_keywords(label, "вкладка", None) {
                        picked = Some(Pick::SwitchTab(i));
                    }
                }
            });
        }
        cmd.group("Окружения", |cmd| {
            if active_env.is_some() && cmd.item("Без окружения", None) {
                picked = Some(Pick::Environment(None));
            }
            for (label, id) in &environments {
                if active_env != Some(*id) && cmd.item(label, None) {
                    picked = Some(Pick::Environment(Some(*id)));
                }
            }
            if cmd.item_with_keywords("Редактировать окружения…", "переменные глобальные", None)
            {
                picked = Some(Pick::EditEnvironments);
            }
        });
        cmd.group("Коллекции", |cmd| {
            if cmd.item("Новая коллекция…", None) {
                picked = Some(Pick::NewCollection);
            }
            if cmd.item_with_keywords("Импорт cURL или Postman…", "import", None) {
                picked = Some(Pick::Import);
            }
            if cmd.item("Показать историю", None) {
                picked = Some(Pick::History);
            }
        });
    });

    match picked {
        Some(Pick::Send) => action.send = true,
        Some(Pick::Save) => action.save = true,
        Some(Pick::CopyCurl) => action.copy_curl = true,
        Some(Pick::NewTab) => {
            state.open_tab(Default::default());
            action.changed = true;
        }
        Some(Pick::CloseTab) => {
            state.close_tab(state.active);
            action.changed = true;
        }
        Some(Pick::SwitchTab(i)) => {
            state.active = i;
            action.changed = true;
        }
        Some(Pick::OpenSaved(id)) => {
            state.open_saved(id);
            action.changed = true;
        }
        Some(Pick::Environment(id)) => {
            state.active_env = id;
            action.changed = true;
        }
        Some(Pick::EditEnvironments) => state.show_env_editor = true,
        Some(Pick::NewCollection) => {
            state.dialog = Some(Dialog::Create { parent: None, name: "Новая коллекция".to_owned() })
        }
        Some(Pick::Import) => state.dialog = Some(Dialog::Import { text: String::new() }),
        Some(Pick::History) => state.sidebar_section = 1,
        None => {}
    }
    action
}
