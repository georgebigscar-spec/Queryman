//! Панель ответа: статус, время, размер, тело и заголовки (FR-20…FR-26).

use super::components;
use crate::state::{AppState, TabResponse};
use eframe::egui::{self, RichText};
use queryman_core::model::ResponseData;

/// Возвращает тело и предлагаемое имя файла, если пользователь нажал «Сохранить в файл».
pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> Option<(Vec<u8>, String)> {
    let mut save = None;
    egui::Panel::bottom("response").resizable(true).default_size(360.0).min_size(120.0).show(ui, |ui| {
        ui.add_space(6.0);
        let tab = state.active_tab_mut();

        if let Some(started) = tab.in_flight {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Выполняется… {:.1} с", started.elapsed().as_secs_f32()));
                components::muted(ui, "Esc — отменить");
            });
            return;
        }

        match &tab.response {
            None => {
                ui.centered_and_justified(|ui| components::muted(ui, "Отправьте запрос — Ctrl+Enter"));
            }
            Some(TabResponse::Err(error)) => {
                ui.label(RichText::new("Запрос не выполнен").strong().color(components::ERROR_COLOR));
                ui.add_space(4.0);
                ui.label(error.to_string());
            }
            Some(TabResponse::Ok { data, text }) => {
                let color = components::status_color(data.status);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(" {} {} ", data.status, data.reason))
                            .strong()
                            .color(color)
                            .background_color(color.gamma_multiply(0.15)),
                    );
                    ui.label(format!("{} мс", data.elapsed_ms))
                        .on_hover_text(format!("До заголовков: {} мс", data.ttfb_ms));
                    ui.label(components::human_size(data.size_bytes));
                    ui.label(RichText::new(&data.http_version).weak());
                    if data.final_url.is_empty() {
                        return;
                    }
                    ui.label(RichText::new(&data.final_url).weak())
                        .on_hover_text("Итоговый адрес после перенаправлений");
                });
                ui.add_space(4.0);

                let headers_label = format!("Headers ({})", data.headers.len());
                components::tabs(ui, &mut tab.response_section, &["Body", headers_label.as_str()]);
                ui.add_space(6.0);

                if tab.response_section == 0 {
                    let mut view = if tab.pretty_view { 0 } else { 1 };
                    ui.horizontal(|ui| {
                        components::toggle_group(ui, &mut view, &["Pretty", "Raw"]);
                        let shown: &str = match (&text.pretty, view) {
                            (Some(pretty), 0) => pretty,
                            _ => &text.raw,
                        };
                        if components::ghost(ui, "Копировать").clicked() {
                            ui.ctx().copy_text(shown.to_owned());
                        }
                        if components::ghost(ui, "Сохранить в файл").on_hover_text("Тело ответа целиком").clicked()
                        {
                            save = Some((data.body.clone(), suggested_file_name(data)));
                        }
                        if text.truncated {
                            components::muted(ui, "Показан первый 1 МБ. Ответ целиком — «Сохранить в файл».");
                        }
                    });
                    tab.pretty_view = view == 0;

                    let mut shown: &str = match (&text.pretty, tab.pretty_view) {
                        (Some(pretty), true) => pretty,
                        _ => &text.raw,
                    };
                    egui::ScrollArea::both().id_salt(("response_body", tab.id)).auto_shrink([false, false]).show(
                        ui,
                        |ui| {
                            ui.add(egui::TextEdit::multiline(&mut shown).code_editor().desired_width(f32::INFINITY));
                        },
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt(("response_headers", tab.id))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("response_headers_grid")
                                .num_columns(2)
                                .striped(true)
                                .spacing([16.0, 4.0])
                                .show(ui, |ui| {
                                    for (name, value) in &data.headers {
                                        ui.label(RichText::new(name).strong());
                                        ui.add(egui::Label::new(value.as_str()).wrap());
                                        ui.end_row();
                                    }
                                });
                        });
                }
            }
        }
    });
    save
}

/// Имя файла из последнего сегмента пути и расширение по Content-Type.
fn suggested_file_name(data: &ResponseData) -> String {
    let stem = url::Url::parse(&data.final_url)
        .ok()
        .and_then(|u| u.path_segments().and_then(|mut s| s.next_back().map(str::to_owned)))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "response".to_owned());
    if stem.contains('.') {
        return stem;
    }
    let ct = data.content_type().unwrap_or_default().to_ascii_lowercase();
    let ext = if ct.contains("json") {
        "json"
    } else if ct.contains("html") {
        "html"
    } else if ct.contains("xml") {
        "xml"
    } else if ct.contains("javascript") {
        "js"
    } else if ct.contains("csv") {
        "csv"
    } else if ct.starts_with("text/") {
        "txt"
    } else if let Some(sub) = ct.strip_prefix("image/") {
        sub.split([';', '+']).next().unwrap_or("bin")
    } else if ct.contains("pdf") {
        "pdf"
    } else {
        "bin"
    };
    format!("{stem}.{ext}")
}
