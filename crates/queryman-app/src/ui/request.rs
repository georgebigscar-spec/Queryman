//! Вкладки, строка URL и редактор запроса: Params, Headers, Body, Auth, Settings.

use super::{components, kv};
use crate::state::AppState;
use eframe::egui;
use queryman_core::model::{ApiKeyLocation, Auth, Body, Method, RawKind, RequestSettings};
use queryman_core::url_sync;
use queryman_http::DEFAULT_TIMEOUT_MS;

#[derive(Default)]
pub struct BarAction {
    pub send: bool,
    pub cancel: bool,
    pub copy_curl: bool,
    pub changed: bool,
}

pub fn show_bar(ctx: &egui::Context, state: &mut AppState) -> BarAction {
    let mut action = BarAction::default();
    egui::TopBottomPanel::top("request_bar").show(ctx, |ui| {
        ui.add_space(6.0);

        // Полоса вкладок (FR-32).
        let mut select = None;
        let mut close = None;
        let mut new_tab = false;
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in state.tabs.iter().enumerate() {
                let label = format!("{}  {}", tab.request.method, tab.request.name);
                let response = ui.selectable_label(i == state.active, label);
                if response.clicked() {
                    select = Some(i);
                }
                if response.middle_clicked() {
                    close = Some(i);
                }
                if ui.small_button("×").on_hover_text("Закрыть (Ctrl+W)").clicked() {
                    close = Some(i);
                }
                ui.add_space(6.0);
            }
            if ui.small_button("+").on_hover_text("Новая вкладка (Ctrl+T)").clicked() {
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

            let buttons_width = 230.0;
            let url_width = (ui.available_width() - buttons_width).max(200.0);
            let url = components::input(ui, &mut tab.request.url, "https://api.example.com/users", url_width);
            if url.changed() {
                tab.request.query = url_sync::params_from_url(&tab.request.url, &tab.request.query);
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
            if components::ghost(ui, "cURL").on_hover_text("Скопировать как cURL").clicked() {
                action.copy_curl = true;
            }
        });

        ui.horizontal(|ui| {
            components::muted(ui, "Название");
            action.changed |= ui
                .add(egui::TextEdit::singleline(&mut tab.request.name).desired_width(320.0))
                .changed();
        });
        ui.add_space(4.0);
    });
    action
}

pub fn show_editor(ctx: &egui::Context, state: &mut AppState) -> bool {
    let mut changed = false;
    egui::CentralPanel::default().show(ctx, |ui| {
        let tab = state.active_tab_mut();
        let headers_label = format!("Headers ({})", tab.request.headers.iter().filter(|h| h.enabled && !h.key.is_empty()).count());
        components::tabs(ui, &mut tab.section, &["Params", headers_label.as_str(), "Body", "Auth", "Settings"]);
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .id_salt(("editor", tab.id))
            .auto_shrink([false, false])
            .show(ui, |ui| match tab.section {
                0 => {
                    if kv::table(ui, "params", &mut tab.request.query, "Параметр", "Значение") {
                        tab.request.url = url_sync::url_from_params(&tab.request.url, &tab.request.query);
                        changed = true;
                    }
                }
                1 => changed |= kv::table(ui, "headers", &mut tab.request.headers, "Заголовок", "Значение"),
                2 => changed |= body_editor(ui, &mut tab.request.body),
                3 => changed |= auth_editor(ui, &mut tab.request.auth),
                _ => changed |= settings_editor(ui, &mut tab.request.settings),
            });
    });
    changed
}

fn body_editor(ui: &mut egui::Ui, body: &mut Body) -> bool {
    let mut changed = false;
    let before = match body {
        Body::None => 0,
        Body::Raw { .. } => 1,
        Body::UrlEncoded(_) => 2,
    };
    let mut kind = before;
    ui.horizontal(|ui| {
        ui.radio_value(&mut kind, 0, "none");
        ui.radio_value(&mut kind, 1, "raw");
        ui.radio_value(&mut kind, 2, "x-www-form-urlencoded");
    });
    if kind != before {
        *body = match kind {
            1 => Body::Raw { text: String::new(), kind: RawKind::Json },
            2 => Body::UrlEncoded(Vec::new()),
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
                .add(
                    egui::TextEdit::multiline(text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(16),
                )
                .changed();
        }
        Body::UrlEncoded(fields) => changed |= kv::table(ui, "urlencoded", fields, "Поле", "Значение"),
    }
    changed
}

fn auth_editor(ui: &mut egui::Ui, auth: &mut Auth) -> bool {
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
        Auth::Inherit => components::muted(ui, "Берётся у папки или коллекции. Вне коллекции запрос уходит без авторизации."),
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
        ui.colored_label(components::ERROR_COLOR, "Проверка сертификата отключена — соединение может быть небезопасным.");
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
