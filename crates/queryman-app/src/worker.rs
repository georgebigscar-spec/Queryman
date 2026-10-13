//! Рабочий поток с tokio: сеть и диск (раздел 3). UI общается с ним только командами.

use eframe::egui;
use queryman_core::model::ResponseData;
use queryman_core::model::Variable;
use queryman_core::prepare::prepare;
use queryman_core::protocol::{Command, Event, HttpError, ResponseText, SendJob, TabId};
use queryman_core::script::{LogLevel, LogLine, ScriptReport, VarOp, VarScope};
use queryman_core::vars::Resolver;
use queryman_http::ClientPool;
use queryman_script::{Phase, ScriptInput, ScriptOutput, ScriptRequest, ScriptResponse};
use queryman_storage::Store;
use std::collections::BTreeMap;
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
            Command::Send { tab, job } => {
                // Не больше одного запроса на вкладку: новый отменяет предыдущий.
                if let Some(previous) = in_flight.remove(&tab) {
                    previous.abort();
                }
                let client = match pool.get(&job.request.settings) {
                    Ok(client) => client,
                    Err(e) => {
                        emit(
                            &events,
                            &ctx,
                            Event::ResponseReady {
                                tab,
                                result: Err(e),
                                text: ResponseText::default(),
                                scripts: ScriptReport::default(),
                            },
                        );
                        continue;
                    }
                };
                let events = events.clone();
                let ctx = ctx.clone();
                let handle = tokio::spawn(async move {
                    let (result, scripts) = send(&client, *job).await;
                    let text = result.as_ref().map(render_text).unwrap_or_default();
                    emit(&events, &ctx, Event::ResponseReady { tab, result, text, scripts });
                });
                in_flight.insert(tab, handle);
            }
            Command::Cancel { tab } => {
                if let Some(handle) = in_flight.remove(&tab) {
                    handle.abort();
                    emit(
                        &events,
                        &ctx,
                        Event::ResponseReady {
                            tab,
                            result: Err(HttpError::Cancelled),
                            text: ResponseText::default(),
                            scripts: ScriptReport::default(),
                        },
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

/// Скрипт pre-request → подстановка переменных → запрос → скрипт tests.
async fn send(client: &reqwest::Client, mut job: SendJob) -> (Result<ResponseData, HttpError>, ScriptReport) {
    let mut report = ScriptReport { environment: job.environment.as_ref().map(|e| e.id), ..Default::default() };

    let pre = job.request.scripts.pre_request.clone();
    if !pre.trim().is_empty() {
        let input = script_input(&job, Phase::Pre, None);
        let out = run_script(pre, input).await;
        out.request.apply_to(&mut job.request);
        absorb(&mut report, &mut job, out, "pre-request");
    }

    let resolved = {
        let env: &[Variable] = job.environment.as_ref().map(|e| e.variables.as_slice()).unwrap_or(&[]);
        let resolver = Resolver::new().with_layer(env).with_layer(&job.globals);
        prepare(&job.request, &resolver, job.parent_auth.as_ref())
    };
    let result = queryman_http::execute(client, resolved).await;

    let test = job.request.scripts.test.clone();
    if let (Ok(data), false) = (&result, test.trim().is_empty()) {
        let response = ScriptResponse {
            code: data.status,
            status: data.reason.clone(),
            response_time: data.elapsed_ms,
            size: data.size_bytes,
            headers: data
                .headers
                .iter()
                .map(|(k, v)| queryman_script::Header { key: k.clone(), value: v.clone() })
                .collect(),
            body: String::from_utf8_lossy(&data.body[..data.body.len().min(SCRIPT_BODY_LIMIT)]).into_owned(),
        };
        let input = script_input(&job, Phase::Test, Some(response));
        let out = run_script(test, input).await;
        absorb(&mut report, &mut job, out, "tests");
    }
    (result, report)
}

/// Сколько тела ответа видит скрипт tests.
const SCRIPT_BODY_LIMIT: usize = 10 * 1024 * 1024;

fn script_input(job: &SendJob, phase: Phase, response: Option<ScriptResponse>) -> ScriptInput {
    let map = |vars: &[Variable]| -> BTreeMap<String, String> {
        vars.iter()
            .filter(|v| v.enabled && !v.key.trim().is_empty())
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect()
    };
    ScriptInput {
        phase,
        request_name: job.request.name.clone(),
        environment_name: job.environment.as_ref().map(|e| e.name.clone()),
        request: ScriptRequest::from_request(&job.request),
        environment: job.environment.as_ref().map(|e| map(&e.variables)).unwrap_or_default(),
        globals: map(&job.globals),
        response,
    }
}

async fn run_script(script: String, input: ScriptInput) -> ScriptOutput {
    let request = input.request.clone();
    tokio::task::spawn_blocking(move || queryman_script::run(&script, &input, queryman_script::DEFAULT_TIMEOUT))
        .await
        .unwrap_or_else(|e| ScriptOutput {
            request,
            ops: Vec::new(),
            tests: Vec::new(),
            logs: Vec::new(),
            error: Some(e.to_string()),
        })
}

/// Добавляет результат скрипта в отчёт и применяет изменения переменных к копиям в задаче,
/// чтобы их увидели подстановка и следующий скрипт.
fn absorb(report: &mut ScriptReport, job: &mut SendJob, out: ScriptOutput, phase: &str) {
    for op in &out.ops {
        match op.scope {
            VarScope::Environment => match &mut job.environment {
                Some(env) => apply_op(&mut env.variables, op),
                None => report.logs.push(LogLine {
                    level: LogLevel::Warn,
                    text: format!("{phase}: окружение не выбрано, изменение «{}» не сохранено", op.key),
                }),
            },
            VarScope::Globals => apply_op(&mut job.globals, op),
        }
    }
    report.ops.extend(out.ops.into_iter().filter(|op| op.scope == VarScope::Globals || job.environment.is_some()));
    report.tests.extend(out.tests);
    report.logs.extend(out.logs);
    if let Some(error) = out.error {
        report.errors.push(format!("{phase}: {error}"));
    }
}

/// Записывает изменение в список переменных: обновляет первую совпадающую или добавляет новую.
pub fn apply_op(vars: &mut Vec<Variable>, op: &VarOp) {
    match &op.value {
        Some(value) => match vars.iter_mut().find(|v| v.key == op.key) {
            Some(var) => {
                var.value = value.clone();
                var.enabled = true;
            }
            None => vars.push(Variable::new(op.key.clone(), value.clone())),
        },
        None => vars.retain(|v| v.key != op.key),
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

#[cfg(test)]
mod tests {
    use super::*;
    use queryman_core::model::{Environment, KeyValue, Request};
    use queryman_core::script::Scripts;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn scripts_run_around_the_request() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users"))
            .and(header("x-trace", "abc"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(r#"{"token":"t-1"}"#, "application/json"))
            .mount(&server)
            .await;

        let env = Environment { name: "dev".into(), ..Default::default() };
        let job = SendJob {
            request: Request {
                url: "{{host}}/users".into(),
                headers: vec![KeyValue::new("Accept", "*/*")],
                scripts: Scripts {
                    pre_request: format!(
                        "pm.environment.set('host', '{}'); pm.request.headers.add({{key: 'X-Trace', value: 'abc'}});",
                        server.uri()
                    ),
                    test: "pm.test('200', () => pm.response.to.have.status(200));\n\
                           pm.environment.set('token', pm.response.json().token);\n\
                           pm.globals.set('seen', pm.environment.get('host') ? 'yes' : 'no');"
                        .into(),
                },
                ..Default::default()
            },
            environment: Some(env.clone()),
            globals: Vec::new(),
            parent_auth: None,
        };
        let client = ClientPool::new().get(&job.request.settings).unwrap();
        let (result, report) = send(&client, job).await;

        assert_eq!(result.unwrap().status, 200);
        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(report.environment, Some(env.id));
        assert!(report.tests.iter().all(|t| t.passed) && report.tests.len() == 1);
        let keys: Vec<&str> = report.ops.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(keys, ["host", "token", "seen"]);
        assert_eq!(report.ops[2].value.as_deref(), Some("yes"));
    }

    #[test]
    fn apply_op_sets_and_unsets() {
        let mut vars = vec![Variable { enabled: false, ..Variable::new("a", "1") }];
        apply_op(&mut vars, &VarOp { scope: VarScope::Globals, key: "a".into(), value: Some("2".into()) });
        apply_op(&mut vars, &VarOp { scope: VarScope::Globals, key: "b".into(), value: Some("3".into()) });
        assert_eq!((vars[0].value.as_str(), vars[0].enabled, vars.len()), ("2", true, 2));
        apply_op(&mut vars, &VarOp { scope: VarScope::Globals, key: "a".into(), value: None });
        assert_eq!(vars.len(), 1);
    }
}
