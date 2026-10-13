//! Единственное место, где вызывается shadcn_egui (риск из раздела 10:
//! API крейта может меняться, правки должны затрагивать только этот файл).

use eframe::egui::{self, Color32, RichText};
use queryman_core::model::Method;
use shadcn_egui as sc;

pub fn install_theme(ctx: &egui::Context, dark: bool) {
    if dark {
        sc::Theme::dark().install(ctx);
    } else {
        sc::Theme::light().install(ctx);
    }
}

pub fn show_toasts(ctx: &egui::Context) {
    sc::Toaster::show(ctx);
}

pub fn notify(ctx: &egui::Context, message: &str) {
    sc::toast(ctx, message, None);
}

pub fn notify_error(ctx: &egui::Context, message: &str) {
    sc::toast_error(ctx, message, None);
}

pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(sc::Button::new(text))
}

pub fn outline(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(sc::Button::outline(text))
}

pub fn ghost(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(sc::Button::ghost(text))
}

pub fn input(ui: &mut egui::Ui, value: &mut String, placeholder: &str, width: f32) -> egui::Response {
    ui.add(sc::Input::new(value).placeholder(placeholder).width(width))
}

pub fn secret_input(ui: &mut egui::Ui, value: &mut String, placeholder: &str, width: f32) -> egui::Response {
    ui.add(sc::Input::new(value).placeholder(placeholder).password().width(width))
}

pub fn checkbox(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    ui.add(sc::Checkbox::new(value))
}

pub fn switch(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    ui.add(sc::Switch::new(value).label(label))
}

/// Возвращает `true`, если выбранная вкладка изменилась.
pub fn tabs(ui: &mut egui::Ui, index: &mut usize, labels: &[&str]) -> bool {
    let before = *index;
    sc::tabs(ui, index, labels);
    *index != before
}

pub fn toggle_group(ui: &mut egui::Ui, index: &mut usize, labels: &[&str]) {
    sc::toggle_group(ui, index, labels);
}

pub fn muted(ui: &mut egui::Ui, text: &str) {
    sc::muted(ui, text);
}

pub fn heading(ui: &mut egui::Ui, text: &str) {
    sc::h3(ui, text);
}

/// Цвета методов из раздела 6.
pub fn method_color(method: &Method) -> Color32 {
    match method {
        Method::Get => Color32::from_rgb(34, 197, 94),
        Method::Post => Color32::from_rgb(234, 179, 8),
        Method::Put => Color32::from_rgb(59, 130, 246),
        Method::Patch => Color32::from_rgb(168, 85, 247),
        Method::Delete => Color32::from_rgb(239, 68, 68),
        _ => Color32::from_rgb(148, 163, 184),
    }
}

pub fn method_text(method: &Method) -> RichText {
    RichText::new(method.as_str()).color(method_color(method)).strong().monospace()
}

/// 2xx зелёный, 3xx синий, 4xx жёлтый, 5xx красный (FR-20).
pub fn status_color(status: u16) -> Color32 {
    match status {
        200..=299 => Color32::from_rgb(34, 197, 94),
        300..=399 => Color32::from_rgb(59, 130, 246),
        400..=499 => Color32::from_rgb(234, 179, 8),
        _ => Color32::from_rgb(239, 68, 68),
    }
}

pub const ERROR_COLOR: Color32 = Color32::from_rgb(239, 68, 68);

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["Б", "КБ", "МБ", "ГБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} Б")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub use sc::TreeUi;

/// Дерево с одиночным выбором (`shadcn_egui::Tree`).
pub fn tree<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    id_salt: &str,
    selected: &mut Option<T>,
    add_nodes: impl FnOnce(&mut TreeUi<'_, T>),
) {
    sc::Tree::new(id_salt).default_open_depth(1).show(ui, selected, add_nodes);
}

/// Модальный диалог. Возвращает результат `add_contents` и признак закрытия (Esc, фон, ✕).
pub fn dialog<R>(
    ctx: &egui::Context,
    id_salt: &str,
    title: &str,
    description: Option<&str>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    let mut dialog = sc::Dialog::new(id_salt, title).width(460.0);
    if let Some(text) = description {
        dialog = dialog.description(text);
    }
    let response = dialog.show(ctx, add_contents);
    (response.inner, response.should_close)
}

/// Кнопки диалога, выровненные вправо. Возвращает `Some(true)` для действия, `Some(false)` для отмены.
pub fn dialog_footer(ui: &mut egui::Ui, action: &str, destructive: bool) -> Option<bool> {
    sc::footer(ui, |ui| {
        let button = if destructive { sc::Button::destructive(action) } else { sc::Button::new(action) };
        if ui.add(button).clicked() {
            return Some(true);
        }
        if ui.add(sc::Button::outline("Отмена")).clicked() {
            return Some(false);
        }
        None
    })
}
