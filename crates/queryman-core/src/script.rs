//! Данные скриптов pre-request и tests (FR-60): что передаётся движку и что он возвращает.
//! Сам движок — крейт `queryman-script`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Скрипты запроса. Пустая строка — скрипта нет.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scripts {
    /// Выполняется до подстановки переменных и отправки.
    pub pre_request: String,
    /// Выполняется после получения ответа.
    pub test: String,
}

impl Scripts {
    pub fn is_empty(&self) -> bool {
        self.pre_request.trim().is_empty() && self.test.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VarScope {
    Environment,
    Globals,
}

/// Изменение переменной из скрипта: `value == None` — удаление.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarOp {
    pub scope: VarScope,
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Log,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}

/// Итог выполнения скриптов одного запроса.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScriptReport {
    /// Окружение, к которому относятся изменения `VarScope::Environment`.
    pub environment: Option<Uuid>,
    pub ops: Vec<VarOp>,
    pub tests: Vec<TestResult>,
    pub logs: Vec<LogLine>,
    /// Ошибки выполнения: исключение, синтаксис, превышение времени.
    pub errors: Vec<String>,
}

impl ScriptReport {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty() && self.tests.is_empty() && self.logs.is_empty() && self.errors.is_empty()
    }

    pub fn passed(&self) -> usize {
        self.tests.iter().filter(|t| t.passed).count()
    }
}
