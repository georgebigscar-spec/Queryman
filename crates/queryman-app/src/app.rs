//! Главный цикл приложения: события рабочего потока, горячие клавиши, панели.

use crate::fonts;
use crate::state::{AppState, TabResponse};
use crate::ui::{self, components};
use crate::worker::{self, WorkerHandle};
use eframe::egui;
use queryman_core::model::{Variable, WorkspaceSnapshot};
use queryman_core::prepare::{prepare, ResolvedRequest};
use queryman_core::protocol::{Command, Event};
use queryman_core::vars::Resolver;
use queryman_storage::Store;
use std::time::{Duration, Instant};

/// Задержка автосохранения после последней правки (раздел 7, правило 3).
const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);

pub struct QuerymanApp {
    state: AppState,
    worker: WorkerHandle,
    store: Option<Store>,
    dirty_since: Option<Instant>,
    saved_on_exit: bool,
    /// Последняя известная системная тема: тему приложения меняем вслед за ней.
    system_theme: Option<egui::Theme>,
}

impl QuerymanApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let system_theme = ctx.system_theme();
        components::install_theme(&ctx, system_theme.unwrap_or(egui::Theme::Dark) == egui::Theme::Dark);
        fonts::install(&ctx);

        let (store, snapshot, warnings) = match Store::open_default() {
            Ok(store) => {
                tracing::info!("каталог данных: {}", store.root().display());
                let report = store.load();
                (Some(store), report.snapshot, report.warnings)
            }
            Err(e) => (
                None,
                WorkspaceSnapshot::default(),
                vec![format!("Каталог данных недоступен ({e}). Изменения не будут сохраняться.")],
            ),
        };
        for warning in &warnings {
            tracing::warn!("{warning}");
            components::notify_error(&ctx, warning);
        }

        let worker = worker::spawn(ctx, store.clone());
        Self {
            state: AppState::from_snapshot(snapshot),
            worker,
            store,
            dirty_since: None,
            saved_on_exit: false,
            system_theme,
        }
    }

    fn resolve_active(&self) -> ResolvedRequest {
        let env_vars: &[Variable] = self.state.active_environment().map(|e| e.variables.as_slice()).unwrap_or(&[]);
        let resolver = Resolver::new().with_layer(env_vars).with_layer(&self.state.globals);
        // Коллекций пока нет, поэтому Inherit = без авторизации.
        prepare(&self.state.active_tab().request, &resolver, None)
    }

    fn send_active(&mut self, ctx: &egui::Context) {
        if self.state.active_tab().request.url.trim().is_empty() {
            components::notify_error(ctx, "Введите URL запроса");
            return;
        }
        let request = self.resolve_active();
        let tab = self.state.active_tab_mut();
        tab.in_flight = Some(Instant::now());
        tab.response = None;
        let id = tab.id;
        self.worker.send(Command::Send { tab: id, request });
    }

    fn cancel_active(&mut self) {
        let tab = self.state.active_tab();
        if tab.in_flight.is_some() {
            self.worker.send(Command::Cancel { tab: tab.id });
        }
    }

    fn copy_as_curl(&self, ctx: &egui::Context) {
        let command = queryman_io::curl::to_curl(&self.resolve_active());
        ctx.copy_text(command);
        components::notify(ctx, "Команда cURL скопирована — она может содержать секреты");
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.worker.events.try_recv() {
            match event {
                Event::ResponseReady { tab, result, text } => {
                    if let Some(tab) = self.state.tab_mut(tab) {
                        tab.in_flight = None;
                        tab.response = Some(match result {
                            Ok(data) => TabResponse::Ok { data, text },
                            Err(e) => TabResponse::Err(e),
                        });
                    }
                }
                Event::Saved => {}
                Event::Error { context, message } => {
                    tracing::error!("{context}: {message}");
                    components::notify_error(ctx, &format!("{context}: {message}"));
                }
            }
        }
    }

    fn handle_hotkeys(&mut self, ctx: &egui::Context) -> bool {
        use egui::{Key, Modifiers};
        let in_flight = self.state.active_tab().in_flight.is_some();
        let (send, cancel, new_tab, close_tab) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                in_flight && i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::COMMAND, Key::T),
                i.consume_key(Modifiers::COMMAND, Key::W),
            )
        });
        if send {
            self.send_active(ctx);
        }
        if cancel {
            self.cancel_active();
        }
        if new_tab {
            self.state.open_tab(Default::default());
        }
        if close_tab {
            self.state.close_tab(self.state.active);
        }
        new_tab || close_tab
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        let Some(since) = self.dirty_since else { return };
        let elapsed = since.elapsed();
        if elapsed >= AUTOSAVE_DELAY {
            self.worker.send(Command::Save(Box::new(self.state.snapshot())));
            self.dirty_since = None;
        } else {
            ctx.request_repaint_after(AUTOSAVE_DELAY - elapsed);
        }
    }

    /// При закрытии окна сохраняем синхронно, чтобы не потерять последние правки.
    fn save_on_exit(&mut self) {
        if self.saved_on_exit {
            return;
        }
        self.saved_on_exit = true;
        if let Some(store) = &self.store {
            if let Err(e) = store.save(&self.state.snapshot()) {
                tracing::error!("сохранение при выходе: {e}");
            }
        }
    }
}

impl eframe::App for QuerymanApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events(ctx);
        let system_theme = ctx.system_theme();
        if system_theme.is_some() && system_theme != self.system_theme {
            self.system_theme = system_theme;
            components::install_theme(ctx, system_theme == Some(egui::Theme::Dark));
        }
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        let mut changed = self.handle_hotkeys(ctx);

        // Порядок панелей важен: сначала края, потом центральная область.
        changed |= ui::header::show(root, &mut self.state);
        changed |= ui::sidebar::show(root, &mut self.state);
        let bar = ui::request::show_bar(root, &mut self.state);
        changed |= bar.changed;
        ui::response::show(root, &mut self.state);
        changed |= ui::request::show_editor(root, &mut self.state);
        changed |= ui::environments::show(ctx, &mut self.state);
        components::show_toasts(ctx);

        if bar.send {
            self.send_active(ctx);
        }
        if bar.cancel {
            self.cancel_active();
        }
        if bar.copy_curl {
            self.copy_as_curl(ctx);
        }

        if changed {
            self.dirty_since.get_or_insert_with(Instant::now);
        }
        self.autosave(ctx);

        // Таймер «Выполняется… N с» обновляется, только пока есть запросы в работе.
        if self.state.any_in_flight() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            self.save_on_exit();
        }
    }
}
