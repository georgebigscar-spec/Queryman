//! Скрипты pre-request и tests на встроенном QuickJS (FR-60).
//!
//! Скрипт видит объект `pm` — подмножество API Postman: `pm.environment`,
//! `pm.globals`, `pm.variables`, `pm.request`, `pm.response`, `pm.test`,
//! `pm.expect`, а также `console`. Обмен с движком идёт строками JSON,
//! вся логика `pm` написана на JavaScript в `prelude.js`.
//! Каждый запуск получает свой рантайм с ограничением памяти и времени.

use queryman_core::model::{Body, KeyValue, Method, RawKind, Request};
use queryman_core::script::{LogLine, TestResult, VarOp};
use rquickjs::{Context, Runtime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const PRELUDE: &str = include_str!("prelude.js");
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// Время на один скрипт по умолчанию.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Pre,
    Test,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub key: String,
    pub value: String,
}

/// Запрос в том виде, в каком его видит и меняет скрипт (до подстановки переменных).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Option<String>,
}

impl ScriptRequest {
    /// Включённые заголовки и raw-тело запроса.
    pub fn from_request(request: &Request) -> Self {
        Self {
            method: request.method.as_str().to_owned(),
            url: request.url.clone(),
            headers: request
                .headers
                .iter()
                .filter(|h| h.enabled && !h.key.trim().is_empty())
                .map(|h| Header { key: h.key.clone(), value: h.value.clone() })
                .collect(),
            body: match &request.body {
                Body::Raw { text, .. } => Some(text.clone()),
                _ => None,
            },
        }
    }

    /// Переносит правки скрипта в копию запроса, которая уйдёт в сеть.
    pub fn apply_to(&self, request: &mut Request) {
        if request.method.as_str() != self.method {
            request.method = Method::STANDARD
                .iter()
                .find(|m| m.as_str() == self.method)
                .cloned()
                .unwrap_or_else(|| Method::Custom(self.method.clone()));
        }
        if request.url != self.url {
            request.url = self.url.clone();
            request.query = queryman_core::url_sync::params_from_url(&request.url, &request.query);
        }
        request.headers = self.headers.iter().map(|h| KeyValue::new(h.key.clone(), h.value.clone())).collect();
        match (&mut request.body, &self.body) {
            (Body::Raw { text, .. }, Some(new)) => *text = new.clone(),
            (Body::Raw { .. }, None) => request.body = Body::None,
            (_, Some(new)) => request.body = Body::Raw { text: new.clone(), kind: RawKind::Text },
            (_, None) => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScriptResponse {
    pub code: u16,
    pub status: String,
    pub response_time: u64,
    pub size: u64,
    pub headers: Vec<Header>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptInput {
    pub phase: Phase,
    pub request_name: String,
    pub environment_name: Option<String>,
    pub request: ScriptRequest,
    pub environment: BTreeMap<String, String>,
    pub globals: BTreeMap<String, String>,
    pub response: Option<ScriptResponse>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScriptOutput {
    pub request: ScriptRequest,
    pub ops: Vec<VarOp>,
    pub tests: Vec<TestResult>,
    pub logs: Vec<LogLine>,
    /// Исключение, синтаксическая ошибка или превышение времени.
    pub error: Option<String>,
}

impl ScriptOutput {
    fn failed(request: &ScriptRequest, error: String) -> Self {
        Self { request: request.clone(), ops: Vec::new(), tests: Vec::new(), logs: Vec::new(), error: Some(error) }
    }
}

/// Выполняет скрипт. Никогда не паникует: любая ошибка попадает в `ScriptOutput::error`.
pub fn run(script: &str, input: &ScriptInput, timeout: Duration) -> ScriptOutput {
    match run_inner(script, input, timeout) {
        Ok(out) => out,
        Err(error) => ScriptOutput::failed(&input.request, error),
    }
}

fn run_inner(script: &str, input: &ScriptInput, timeout: Duration) -> Result<ScriptOutput, String> {
    let input_json = serde_json::to_string(input).map_err(|e| e.to_string())?;
    let runtime = Runtime::new().map_err(|e| format!("не удалось запустить JavaScript: {e}"))?;
    runtime.set_memory_limit(MEMORY_LIMIT);
    let deadline = Instant::now() + timeout;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let context = Context::full(&runtime).map_err(|e| format!("не удалось запустить JavaScript: {e}"))?;

    let output_json = context.with(|ctx| -> Result<String, String> {
        let describe = |e: rquickjs::Error| match e {
            rquickjs::Error::Exception => {
                let caught = ctx.catch();
                caught
                    .as_exception()
                    .and_then(|ex| ex.message())
                    .or_else(|| caught.as_string().and_then(|s| s.to_string().ok()))
                    .unwrap_or_else(|| "исключение в скрипте".to_owned())
            }
            other => other.to_string(),
        };
        let globals = ctx.globals();
        globals.set("__input", input_json.as_str()).map_err(describe)?;
        globals.set("__script", script).map_err(describe)?;
        ctx.eval::<(), _>(PRELUDE).map_err(describe)?;
        if let Err(e) = ctx.eval::<(), _>("__run()") {
            let message = describe(e);
            // Прерывание по таймеру нельзя перехватить в JS, поэтому оно приходит сюда.
            return Err(if Instant::now() > deadline {
                format!("скрипт выполнялся дольше {} с и был остановлен", timeout.as_secs_f32())
            } else {
                message
            });
        }
        ctx.eval::<String, _>("__finish()").map_err(describe)
    })?;

    serde_json::from_str(&output_json).map_err(|e| format!("некорректный результат скрипта: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use queryman_core::script::{LogLevel, VarScope};

    fn input(phase: Phase) -> ScriptInput {
        ScriptInput {
            phase,
            request_name: "users".into(),
            environment_name: Some("dev".into()),
            request: ScriptRequest {
                method: "GET".into(),
                url: "{{host}}/users".into(),
                headers: vec![Header { key: "Accept".into(), value: "*/*".into() }],
                body: None,
            },
            environment: BTreeMap::from([("host".to_owned(), "localhost".to_owned())]),
            globals: BTreeMap::from([("token".to_owned(), "g".to_owned())]),
            response: None,
        }
    }

    fn with_response(code: u16, body: &str) -> ScriptInput {
        let mut i = input(Phase::Test);
        i.response = Some(ScriptResponse {
            code,
            status: "OK".into(),
            response_time: 12,
            size: body.len() as u64,
            headers: vec![Header { key: "Content-Type".into(), value: "application/json".into() }],
            body: body.into(),
        });
        i
    }

    #[test]
    fn pre_request_changes_variables_and_request() {
        let script = r#"
            pm.environment.set("ts", 42);
            pm.globals.unset("token");
            pm.variables.set("local", "x");
            pm.request.headers.upsert({ key: "X-Trace", value: pm.variables.get("host") });
            pm.request.headers.remove("accept");
            pm.request.url = pm.request.url + "?page=1";
            console.log("host is", pm.environment.get("host"), { a: 1 });
        "#;
        let out = run(script, &input(Phase::Pre), DEFAULT_TIMEOUT);
        assert_eq!(out.error, None);
        assert_eq!(
            out.ops,
            vec![
                VarOp { scope: VarScope::Environment, key: "ts".into(), value: Some("42".into()) },
                VarOp { scope: VarScope::Globals, key: "token".into(), value: None },
            ]
        );
        assert_eq!(out.request.url, "{{host}}/users?page=1");
        assert_eq!(out.request.headers, vec![Header { key: "X-Trace".into(), value: "localhost".into() }]);
        assert_eq!(out.logs, vec![LogLine { level: LogLevel::Log, text: "host is localhost {\"a\":1}".into() }]);
    }

    #[test]
    fn tests_and_expect() {
        let script = r#"
            pm.test("status 200", () => pm.response.to.have.status(200));
            pm.test("ok", () => pm.response.to.be.ok);
            const data = pm.response.json();
            pm.test("json", () => {
                pm.expect(data.items).to.be.an("array").that.has.lengthOf(3);
                pm.expect(data.items).to.include(2);
                pm.expect(data).to.have.property("ok", true);
                pm.expect(data.name).to.not.equal("x");
                pm.expect(pm.response.responseTime).to.be.below(1000);
            });
            pm.test("fails", () => pm.expect(data.items.length).to.equal(5));
            pm.environment.set("first", data.items[0]);
        "#;
        let out = run(script, &with_response(200, r#"{"ok":true,"items":[1,2,3],"name":"n"}"#), DEFAULT_TIMEOUT);
        assert_eq!(out.error, None);
        let passed: Vec<bool> = out.tests.iter().map(|t| t.passed).collect();
        assert_eq!(passed, [true, true, true, false]);
        assert_eq!(out.tests[3].error.as_deref(), Some("expected 3 to equal 5"));
        assert_eq!(out.ops[0].value.as_deref(), Some("1"));
    }

    #[test]
    fn errors_are_reported() {
        let out = run("throw new Error('boom')", &input(Phase::Pre), DEFAULT_TIMEOUT);
        assert_eq!(out.error.as_deref(), Some("Error: boom"));

        let out = run("this is not js", &input(Phase::Pre), DEFAULT_TIMEOUT);
        assert!(out.error.as_deref().unwrap().starts_with("SyntaxError"), "{:?}", out.error);

        let out = run("console.log('before'); undefinedFn()", &input(Phase::Pre), DEFAULT_TIMEOUT);
        assert_eq!(out.logs.len(), 1);
        assert!(out.error.unwrap().contains("undefinedFn"));
    }

    #[test]
    fn endless_loop_is_stopped() {
        let started = Instant::now();
        let out = run("while (true) {}", &input(Phase::Pre), Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(out.error.unwrap().contains("остановлен"));
    }
}
