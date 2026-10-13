//! Сообщения между UI-потоком и рабочим потоком (раздел 3).

use crate::model::{ResponseData, WorkspaceSnapshot};
use crate::prepare::ResolvedRequest;

pub type TabId = u64;

/// UI → рабочий поток.
#[derive(Debug)]
pub enum Command {
    Send { tab: TabId, request: ResolvedRequest },
    Cancel { tab: TabId },
    Save(Box<WorkspaceSnapshot>),
    Shutdown,
}

/// Текст ответа, подготовленный в рабочем потоке, чтобы UI не форматировал
/// большие тела в кадре отрисовки (NFR-03).
#[derive(Debug, Clone, Default)]
pub struct ResponseText {
    pub raw: String,
    pub pretty: Option<String>,
    /// Показана только часть тела.
    pub truncated: bool,
}

/// Рабочий поток → UI.
#[derive(Debug)]
pub enum Event {
    ResponseReady { tab: TabId, result: Result<ResponseData, HttpError>, text: ResponseText },
    Saved,
    Error { context: String, message: String },
}

/// Сетевые ошибки в понятном виде (FR-26).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HttpError {
    #[error("Некорректный URL: {0}")]
    InvalidUrl(String),
    #[error("Некорректный запрос: {0}")]
    InvalidRequest(String),
    #[error("Не удалось подключиться: {0}")]
    Connect(String),
    #[error("Превышено время ожидания ({0} мс)")]
    Timeout(u64),
    #[error("Слишком много перенаправлений")]
    TooManyRedirects,
    #[error("Запрос отменён")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}
