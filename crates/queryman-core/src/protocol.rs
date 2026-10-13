//! Сообщения между UI-потоком и рабочим потоком (раздел 3).

use crate::model::{Auth, Environment, Request, ResponseData, Variable, WorkspaceSnapshot};
use crate::script::ScriptReport;

pub type TabId = u64;

/// UI → рабочий поток.
#[derive(Debug)]
pub enum Command {
    Send {
        tab: TabId,
        job: Box<SendJob>,
    },
    Cancel {
        tab: TabId,
    },
    Save(Box<WorkspaceSnapshot>),
    /// Спросить путь и записать тело ответа целиком (FR-24).
    SaveResponse {
        body: Vec<u8>,
        file_name: String,
    },
    /// Выбрать файл для тела запроса во вкладке `tab`.
    PickFile {
        tab: TabId,
        target: FileTarget,
    },
    /// Выбрать файл для импорта и прочитать его как текст.
    PickImportFile,
    Shutdown,
}

/// Куда подставить выбранный файл.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTarget {
    /// Строка form-data с этим индексом.
    FormField(usize),
    Binary,
}

/// Всё, что нужно рабочему потоку, чтобы выполнить скрипты, подставить переменные и отправить запрос.
#[derive(Debug, Clone)]
pub struct SendJob {
    pub request: Request,
    pub environment: Option<Environment>,
    pub globals: Vec<Variable>,
    /// Авторизация папки или коллекции для режима Inherit.
    pub parent_auth: Option<Auth>,
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
// Событий в полёте единицы, поэтому крупный вариант не упаковывается в Box.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Event {
    ResponseReady {
        tab: TabId,
        result: Result<ResponseData, HttpError>,
        text: ResponseText,
        scripts: ScriptReport,
    },
    Saved,
    FilePicked {
        tab: TabId,
        target: FileTarget,
        path: String,
    },
    /// Содержимое файла, выбранного для импорта.
    ImportFileLoaded(String),
    /// Сообщение для пользователя об успешном действии.
    Notice(String),
    Error {
        context: String,
        message: String,
    },
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
