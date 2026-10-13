//! Вкладки, строка URL и редактор запроса: Params, Headers, Body, Auth, Settings.

use super::{components, kv};
use crate::state::AppState;
use eframe::egui;
use queryman_core::model::{ApiKeyLocation, Auth, Body, FormField, Method, RawKind, Request, RequestSettings};
use queryman_core::protocol::{Command, FileTarget};
use queryman_core::script::Scripts;
use queryman_core::url_sync;
use queryman_http::DEFAULT_TIMEOUT_MS;

#[derive(Default)]
pub struct BarAction {
    pub send: bool,
    pub cancel: bool,
    pub copy_curl: bool,
    pub save: bool,
    pub changed: bool,
}

pub fn show_bar(ui: &mut egui::Ui, state: &mut AppState) -> BarAction {
    let mut action = BarAction::default();
    egui::Panel::top("request_bar").show(ui, |ui| {
        ui.add_space(6.0);

        // Полоса вкладок (FR-32).
        let mut select = None;
        let mut close = None;
        let mut new_tab = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, tab) in state.tabs.iter().enumerate() {
                let marker = if state.is_dirty(tab) { " *" } else { "" };
                let title = format!("{}{marker}", tab.request.name);
                let event = components::request_tab(ui, tab.id, &tab.request.method, &title, i == state.active);
                if event.select {
                    select = Some(i);
                }
                if event.close {
                    close = Some(i);
                }
            }
            if components::ghost(ui, "+").on_hover_text("Новая вкладка (Ctrl+T)").clicked() {
                new_tab = true;
            }
        });
        if let Some(i) = select {
            state.active = i;
            action.changed = true;
        }
        if let Some(i) = close {
            state.close_tab(i);
            action.changed = true;
        }
        if new_tab {
            state.open_tab(Default::default());
            action.changed = true;
        }
        ui.separator();

        // Строка запроса.
        let linked = state.saved_request(state.active_tab().request.id).is_some();
        let tab = state.active_tab_mut();
        ui.horizontal(|ui| {
            let before = tab.request.method.clone();
            egui::ComboBox::from_id_salt(("method", tab.id))
                .selected_text(components::method_text(&tab.request.method))
                .width(96.0)
                .show_ui(ui, |ui| {
                    for method in Method::STANDARD.iter() {
                        ui.selectable_value(&mut tab.request.method, method.clone(), components::method_text(method));
                    }
                });
            action.changed |= tab.request.method != before;

            let buttons_width = 330.0;
            let url_width = (ui.available_width() - buttons_width).max(200.0);
            let url = components::input(ui, &mut tab.request.url, "https://api.example.com/users", url_width);
            if url.changed() {
                let pasted = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))));
                if pasted && queryman_io::curl::looks_like_curl(&tab.request.url) {
                    // Вставленная команда cURL превращается в запрос (как в Postman).
                    match queryman_io::curl::from_curl(&tab.request.url) {
                        Ok(imported) => {
                            tab.request = Request { id: tab.request.id, ..imported };
                            components::notify(ui.ctx(), "Команда cURL разобрана");
                        }
                        Err(e) => components::notify_error(ui.ctx(), &format!("cURL: {e}")),
                    }
                } else {
                    tab.request.query = url_sync::params_from_url(&tab.request.url, &tab.request.query);
                }
                action.changed = true;
            }
            if url.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                action.send = true;
            }

            if tab.in_flight.is_some() {
                if components::outline(ui, "Отмена").on_hover_text("Esc").clicked() {
                    action.cancel = true;
                }
            } else if components::primary(ui, "Send").on_hover_text("Ctrl+Enter").clicked() {
                action.send = true;
            }
            let save_hint = if linked {
                "Сохранить в коллекцию (Ctrl+S)"
            } else {
                "Сохранить в коллекцию… (Ctrl+S)"
            };
            if components::outline(ui, "Сохранить").on_hover_text(save_hint).clicked() {
                action.save = true;
            }
            if components::ghost(ui, "cURL").on_hover_text("Скопировать как cURL").clicked() {
                action.copy_curl = true;
            }
        });

        ui.horizontal(|ui| {
            components::muted(ui, "Название");
            action.changed |= components::input(ui, &mut tab.request.name, "Название запроса", 320.0).changed();
        });
        ui.add_space(4.0);
    });
    action
}

pub fn show_editor(ui: &mut egui::Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    let mut pick = None;
    egui::CentralPanel::default().show(ui, |ui| {
        let tab = state.active_tab_mut();
        let headers_label =
            format!("Headers ({})", tab.request.headers.iter().filter(|h| h.enabled && !h.key.is_empty()).count());
        let script_count = [&tab.request.scripts.pre_request, &tab.request.scripts.test]
            .iter()
            .filter(|s| !s.trim().is_empty())
            .count();
        let scripts_label = if script_count == 0 { "Scripts".to_owned() } else { format!("Scripts ({script_count})") };
        components::tabs(
            ui,
            &mut tab.section,
            &["Params", headers_label.as_str(), "Body", "Auth", scripts_label.as_str(), "Settings"],
        );
        ui.add_space(8.0);

        egui::ScrollArea::vertical().id_salt(("editor", tab.id)).auto_shrink([false, false]).show(ui, |ui| {
            match tab.section {
                0 => {
                    if kv::table(ui, "params", &mut tab.request.query, "Параметр", "Значение") {
                        tab.request.url = url_sync::url_from_params(&tab.request.url, &tab.request.query);
                        changed = true;
                    }
                }
                1 => changed |= kv::table(ui, "headers", &mut tab.request.headers, "Заголовок", "Значение"),
                2 => {
                    let (body_changed, target) = body_editor(ui, &mut tab.request.body);
                    changed |= body_changed;
                    pick = target.map(|target| Command::PickFile { tab: tab.id, target });
                }
                3 => changed |= auth_editor(ui, &mut tab.request.auth),
                4 => changed |= scripts_editor(ui, &mut tab.script_section, &mut tab.request.scripts),
                _ => changed |= settings_editor(ui, &mut tab.request.settings),
            }
        });
    });
    state.outbox.extend(pick);
    changed
}

/// Возвращает, изменилось ли тело, и файл, который нужно выбрать в системном диалоге.
fn body_editor(ui: &mut egui::Ui, body: &mut Body) -> (bool, Option<FileTarget>) {
    let mut changed = false;
    let mut pick = None;
    let before = match body {
        Body::None => 0,
        Body::Raw { .. } => 1,
        Body::UrlEncoded(_) => 2,
        Body::FormData(_) => 3,
        Body::Binary { .. } => 4,
    };
    let mut kind = before;
    ui.horizontal(|ui| {
        ui.radio_value(&mut kind, 0, "none");
        ui.radio_value(&mut kind, 1, "raw");
        ui.radio_value(&mut kind, 2, "x-www-form-urlencoded");
        ui.radio_value(&mut kind, 3, "form-data");
        ui.radio_value(&mut kind, 4, "binary");
    });
    if kind != before {
        *body = match kind {
            1 => Body::Raw { text: String::new(), kind: RawKind::Json },
            2 => Body::UrlEncoded(Vec::new()),
            3 => Body::FormData(Vec::new()),
            4 => Body::Binary { path: String::new() },
            _ => Body::None,
        };
        changed = true;
    }
    ui.add_space(8.0);

    match body {
        Body::None => components::muted(ui, "У запроса нет тела."),
        Body::Raw { text, kind } => {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("raw_kind").selected_text(kind.label()).show_ui(ui, |ui| {
                    for k in RawKind::ALL {
                        changed |= ui.selectable_value(kind, k, k.label()).changed();
                    }
                });
                if *kind == RawKind::Json && components::ghost(ui, "Форматировать").clicked() {
                    match serde_json::from_str::<serde_json::Value>(text) {
                        Ok(value) => {
                            if let Ok(pretty) = serde_json::to_string_pretty(&value) {
                                *text = pretty;
                                changed = true;
                            }
                        }
                        Err(e) => components::notify_error(ui.ctx(), &format!("Некорректный JSON: {e}")),
                    }
                }
            });
            changed |= ui
                .add(egui::TextEdit::multiline(text).code_editor().desired_width(f32::INFINITY).desired_rows(16))
                .changed();
        }
        Body::UrlEncoded(fields) => changed |= kv::table(ui, "urlencoded", fields, "Поле", "Значение"),
        Body::FormData(fields) => {
            let (form_changed, row) = form_data_table(ui, fields);
            changed |= form_changed;
            pick = row.map(FileTarget::FormField);
        }
        Body::Binary { path } => {
            components::muted(ui, "Файл отправляется как есть. Content-Type по умолчанию: application/octet-stream.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 130.0).max(200.0);
                changed |= components::input(ui, path, "Путь к файлу", width).changed();
                if components::outline(ui, "Выбрать файл…").clicked() {
                    pick = Some(FileTarget::Binary);
                }
            });
        }
    }
    (changed, pick)
}

/// Таблица form-data: у каждой строки тип «Текст» или «Файл».
/// Возвращает, изменилась ли таблица, и строку, для которой нужно выбрать файл.
fn form_data_table(ui: &mut egui::Ui, fields: &mut Vec<FormField>) -> (bool, Option<usize>) {
    if fields.last().is_none_or(|f| !f.is_blank()) {
        fields.push(FormField::default());
    }
    let mut changed = false;
    let mut remove = None;
    let mut pick = None;
    let count = fields.len();
    let column = ((ui.available_width() - 200.0) / 2.0).max(140.0);

    egui::Grid::new("form_data").num_columns(5).spacing([8.0, 6.0]).show(ui, |ui| {
        for (i, field) in fields.iter_mut().enumerate() {
            changed |= components::checkbox(ui, &mut field.enabled).changed();
            changed |= components::input(ui, &mut field.key, "Поле", column).changed();
            let before = field.file;
            egui::ComboBox::from_id_salt(("form_kind", i))
                .selected_text(if field.file { "Файл" } else { "Текст" })
                .width(72.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut field.file, false, "Текст");
                    ui.selectable_value(&mut field.file, true, "Файл");
                });
            if field.file != before {
                field.value.clear();
                changed = true;
            }
            if field.file {
                ui.horizontal(|ui| {
                    changed |= components::input(ui, &mut field.value, "Путь к файлу", column - 40.0).changed();
                    if components::ghost(ui, "…").on_hover_text("Выбрать файл").clicked() {
                        pick = Some(i);
                    }
                });
            } else {
                changed |= components::input(ui, &mut field.value, "Значение", column).changed();
            }
            if i + 1 == count {
                ui.label("");
            } else if components::ghost(ui, "×").on_hover_text("Удалить строку").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });

    if let Some(i) = remove {
        fields.remove(i);
        changed = true;
    }
    (changed, pick)
}

pub fn auth_editor(ui: &mut egui::Ui, auth: &mut Auth) -> bool {
    const LABELS: [&str; 5] = ["Наследовать", "Без авторизации", "API Key", "Bearer Token", "Basic Auth"];
    let mut changed = false;
    let before = match auth {
        Auth::Inherit => 0,
        Auth::None => 1,
        Auth::ApiKey { .. } => 2,
        Auth::Bearer { .. } => 3,
        Auth::Basic { .. } => 4,
    };
    let mut kind = before;
    egui::ComboBox::from_id_salt("auth_kind").selected_text(LABELS[kind]).width(200.0).show_ui(ui, |ui| {
        for (i, label) in LABELS.iter().enumerate() {
            ui.selectable_value(&mut kind, i, *label);
        }
    });
    if kind != before {
        *auth = match kind {
            0 => Auth::Inherit,
            1 => Auth::None,
            2 => Auth::ApiKey { key: String::new(), value: String::new(), location: ApiKeyLocation::Header },
            3 => Auth::Bearer { token: String::new(), prefix: String::new() },
            _ => Auth::Basic { username: String::new(), password: String::new() },
        };
        changed = true;
    }
    ui.add_space(8.0);

    let width = 360.0;
    match auth {
        Auth::Inherit => {
            components::muted(ui, "Берётся у папки или коллекции. Вне коллекции запрос уходит без авторизации.")
        }
        Auth::None => components::muted(ui, "Заголовки авторизации не добавляются."),
        Auth::ApiKey { key, value, location } => {
            egui::Grid::new("auth_api_key").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Ключ");
                changed |= components::input(ui, key, "X-API-Key", width).changed();
                ui.end_row();
                ui.label("Значение");
                changed |= components::secret_input(ui, value, "{{apiKey}}", width).changed();
                ui.end_row();
                ui.label("Куда");
                ui.horizontal(|ui| {
                    changed |= ui.radio_value(location, ApiKeyLocation::Header, "Заголовок").changed();
                    changed |= ui.radio_value(location, ApiKeyLocation::Query, "Query-параметр").changed();
                });
                ui.end_row();
            });
        }
        Auth::Bearer { token, prefix } => {
            egui::Grid::new("auth_bearer").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Токен");
                changed |= components::secret_input(ui, token, "{{token}}", width).changed();
                ui.end_row();
                ui.label("Префикс");
                changed |= components::input(ui, prefix, "Bearer", width).changed();
                ui.end_row();
            });
        }
        Auth::Basic { username, password } => {
            egui::Grid::new("auth_basic").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Логин");
                changed |= components::input(ui, username, "", width).changed();
                ui.end_row();
                ui.label("Пароль");
                changed |= components::secret_input(ui, password, "", width).changed();
                ui.end_row();
            });
        }
    }
    ui.add_space(8.0);
    components::muted(ui, "Заголовок Authorization, заданный вручную во вкладке Headers, важнее этих настроек.");
    changed
}

/// Готовые фрагменты: название кнопки, вкладка (0 — pre-request, 1 — tests), код.
const SNIPPETS: [(&str, usize, &str); 6] = [
    ("Задать переменную окружения", 0, "pm.environment.set(\"name\", \"value\");\n"),
    ("Добавить заголовок", 0, "pm.request.headers.upsert({ key: \"X-Request-Id\", value: pm.variables.replaceIn(\"{{$uuid}}\") });\n"),
    ("Статус 200", 1, "pm.test(\"Статус 200\", () => {\n    pm.response.to.have.status(200);\n});\n"),
    ("Время ответа", 1, "pm.test(\"Ответ быстрее 500 мс\", () => {\n    pm.expect(pm.response.responseTime).to.be.below(500);\n});\n"),
    ("Поле в JSON", 1, "pm.test(\"В ответе есть id\", () => {\n    const data = pm.response.json();\n    pm.expect(data).to.have.property(\"id\");\n});\n"),
    ("Сохранить токен", 1, "const data = pm.response.json();\npm.environment.set(\"token\", data.token);\n"),
];

fn scripts_editor(ui: &mut egui::Ui, section: &mut usize, scripts: &mut Scripts) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        components::toggle_group(ui, section, &["Pre-request", "Tests"]);
        ui.add_space(8.0);
        let hint = if *section == 0 {
            "Выполняется перед отправкой: может менять запрос и переменные."
        } else {
            "Выполняется после ответа: pm.test, pm.expect, pm.response."
        };
        components::muted(ui, hint);
    });
    ui.add_space(6.0);
    let text = if *section == 0 { &mut scripts.pre_request } else { &mut scripts.test };
    ui.horizontal_wrapped(|ui| {
        components::muted(ui, "Вставить:");
        for (label, target, code) in SNIPPETS {
            if target == *section && components::ghost(ui, label).clicked() {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(code);
                changed = true;
            }
        }
    });
    ui.add_space(4.0);
    changed |= ui
        .add(
            egui::TextEdit::multiline(text)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(14)
                .hint_text("// JavaScript, объект pm как в Postman"),
        )
        .changed();
    changed
}

fn settings_editor(ui: &mut egui::Ui, settings: &mut RequestSettings) -> bool {
    let mut changed = false;
    changed |= components::switch(ui, &mut settings.follow_redirects, "Следовать перенаправлениям").changed();
    if settings.follow_redirects {
        ui.horizontal(|ui| {
            ui.label("Не более");
            changed |= ui.add(egui::DragValue::new(&mut settings.max_redirects).range(0..=50)).changed();
            ui.label("перенаправлений");
        });
    }
    ui.add_space(6.0);
    changed |= components::switch(ui, &mut settings.verify_tls, "Проверять TLS-сертификат").changed();
    if !settings.verify_tls {
        ui.colored_label(
            components::ERROR_COLOR,
            "Проверка сертификата отключена — соединение может быть небезопасным.",
        );
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Таймаут");
        let mut timeout = settings.timeout_ms.unwrap_or(0);
        if ui.add(egui::DragValue::new(&mut timeout).range(0..=600_000).speed(100).suffix(" мс")).changed() {
            settings.timeout_ms = (timeout > 0).then_some(timeout);
            changed = true;
        }
        components::muted(ui, &format!("0 — по умолчанию ({DEFAULT_TIMEOUT_MS} мс)"));
    });
    changed
}
