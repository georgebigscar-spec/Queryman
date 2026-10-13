//! Рабочий поток с tokio: сеть и диск (раздел 3). UI общается с ним только командами.

use eframe::egui;
use queryman_core::model::ResponseData;
use queryman_core::protocol::{Command, Event, HttpError, ResponseText, TabId};
use queryman_http::ClientPool;
use queryman_storage::Store;
use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;

/// Тело больше этого размера не форматируется (FR-25).
const PRETTY_LIMIT: usize = 10 * 1024 * 1024;
/// Сколько текста отдаём в UI для показа.
const DISPLAY_LIMIT: usize = 1024 * 1024;

pub struct WorkerHandle {
    commands: mpsc::UnboundedSender<Command>,
    pub events: std_mpsc::Receiver<Event>,
}

impl WorkerHandle {
    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("рабочий поток остановлен, команда потеряна");
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

pub fn spawn(ctx: egui::Context, store: Option<Store>) -> WorkerHandle {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (evt_tx, evt_rx) = std_mpsc::channel();
    std::thread::Builder::new()
        .name("queryman-worker".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("queryman-io")
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    emit(
                        &evt_tx,
                        &ctx,
                        Event::Error {
                            context: "Запуск рабочего потока".into(), message: e.to_string()
                        },
                    );
                    return;
                }
            };
            runtime.block_on(run(cmd_rx, evt_tx, ctx, store));
        })
        .expect("не удалось запустить рабочий поток");
    WorkerHandle { commands: cmd_tx, events: evt_rx }
}

fn emit(events: &std_mpsc::Sender<Event>, ctx: &egui::Context, event: Event) {
    let _ = events.send(event);
    ctx.request_repaint();
}

async fn run(
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: std_mpsc::Sender<Event>,
    ctx: egui::Context,
    store: Option<Store>,
) {
    let mut pool = ClientPool::new();
    let mut in_flight: HashMap<TabId, tokio::task::JoinHandle<()>> = HashMap::new();

    while let Some(command) = commands.recv().await {
        in_flight.retain(|_, handle| !handle.is_finished());
        match command {
            Command::Send { tab, request } => {
                // Не больше одного запроса на вкладку: новый отменяет предыдущий.
                if let Some(previous) = in_flight.remove(&tab) {
                    previous.abort();
                }
                let client = match pool.get(&request.settings) {
                    Ok(client) => client,
                    Err(e) => {
                        emit(
                            &events,
                            &ctx,
                            Event::ResponseReady { tab, result: Err(e), text: ResponseText::default() },
                        );
                        continue;
                    }
                };
                let events = events.clone();
                let ctx = ctx.clone();
                let handle = tokio::spawn(async move {
                    let result = queryman_http::execute(&client, request).await;
                    let text = result.as_ref().map(render_text).unwrap_or_default();
                    emit(&events, &ctx, Event::ResponseReady { tab, result, text });
                });
                in_flight.insert(tab, handle);
            }
            Command::Cancel { tab } => {
                if let Some(handle) = in_flight.remove(&tab) {
                    handle.abort();
                    emit(
                        &events,
                        &ctx,
                        Event::ResponseReady { tab, result: Err(HttpError::Cancelled), text: ResponseText::default() },
                    );
                }
            }
            Command::Save(snapshot) => {
                let Some(store) = store.clone() else { continue };
                let outcome = tokio::task::spawn_blocking(move || store.save(&snapshot)).await;
                let event = match outcome {
                    Ok(Ok(())) => Event::Saved,
                    Ok(Err(e)) => Event::Error { context: "Сохранение".into(), message: e.to_string() },
                    Err(e) => Event::Error { context: "Сохранение".into(), message: e.to_string() },
                };
                emit(&events, &ctx, event);
            }
            Command::SaveResponse { body, file_name } => {
                let events = events.clone();
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let Some(file) = rfd::AsyncFileDialog::new().set_file_name(&file_name).save_file().await else {
                        return; // пользователь закрыл диалог
                    };
                    let path = file.path().to_owned();
                    let size = body.len() as u64;
                    let outcome = tokio::task::spawn_blocking(move || std::fs::write(&path, body).map(|()| path)).await;
                    let event = match outcome {
                        Ok(Ok(path)) => Event::Notice(format!("Ответ сохранён: {} ({size} Б)", path.display())),
                        Ok(Err(e)) => {
                            Event::Error { context: "Сохранение ответа".into(), message: e.to_string() }
                        }
                        Err(e) => {
                            Event::Error { context: "Сохранение ответа".into(), message: e.to_string() }
                        }
                    };
                    emit(&events, &ctx, event);
                });
            }
            Command::Shutdown => break,
        }
    }
    for (_, handle) in in_flight {
        handle.abort();
    }
}

/// Готовит текст для показа: обрезка и форматирование JSON выполняются здесь,
/// а не в кадре отрисовки.
fn render_text(data: &ResponseData) -> ResponseText {
    let truncated = data.body.len() > DISPLAY_LIMIT;
    let shown = &data.body[..data.body.len().min(DISPLAY_LIMIT)];
    let raw = String::from_utf8_lossy(shown).into_owned();
    let pretty = (data.body.len() <= PRETTY_LIMIT && looks_like_json(data))
        .then(|| serde_json::from_slice::<serde_json::Value>(&data.body).ok())
        .flatten()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .map(|text| truncate_utf8(text, DISPLAY_LIMIT));
    ResponseText { raw, pretty, truncated }
}

fn looks_like_json(data: &ResponseData) -> bool {
    data.is_json() || matches!(data.body.iter().find(|b| !b.is_ascii_whitespace()), Some(b'{') | Some(b'['))
}

fn truncate_utf8(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut cut = max;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    text
}
