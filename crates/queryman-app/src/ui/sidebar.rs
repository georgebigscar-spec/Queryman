//! Боковая панель. Дерево коллекций на shadcn_egui::Tree — этап MVP (FR-30).

use super::components;
use crate::state::AppState;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> bool {
    let mut select = None;
    let mut new_tab = false;
    egui::Panel::left("sidebar").resizable(true).default_size(240.0).size_range(180.0..=420.0).show(ui, |ui| {
        ui.add_space(8.0);
        components::heading(ui, "Коллекции");
        components::muted(ui, "Дерево коллекций появится на этапе MVP.");
        ui.separator();
        components::heading(ui, "Открытые запросы");
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, tab) in state.tabs.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(components::method_text(&tab.request.method));
                    if ui.selectable_label(i == state.active, tab.request.name.as_str()).clicked() {
                        select = Some(i);
                    }
                });
            }
            ui.add_space(8.0);
            if components::ghost(ui, "+ Новый запрос").clicked() {
                new_tab = true;
            }
        });
    });

    if let Some(i) = select {
        state.active = i;
    }
    if new_tab {
        state.open_tab(Default::default());
    }
    select.is_some() || new_tab
}
