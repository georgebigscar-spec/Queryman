//! Доменная модель (раздел 4 спецификации).

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// HTTP-метод запроса.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    /// Произвольный метод, введённый вручную (FR-01).
    #[serde(untagged)]
    Custom(String),
}

impl Method {
    pub const STANDARD: [Method; 7] =
        [Method::Get, Method::Post, Method::Put, Method::Patch, Method::Delete, Method::Head, Method::Options];

    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Custom(m) => m.as_str(),
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Строка таблиц Params, Headers, x-www-form-urlencoded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub description: String,
}

impl Default for KeyValue {
    fn default() -> Self {
        Self { key: String::new(), value: String::new(), enabled: true, description: String::new() }
    }
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), ..Self::default() }
    }

    /// Пустая строка-заготовка, которую таблица держит в конце.
    pub fn is_blank(&self) -> bool {
        self.key.trim().is_empty() && self.value.is_empty() && self.description.is_empty()
    }
}

/// Подтип raw-тела, определяет Content-Type по умолчанию.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawKind {
    #[default]
    Json,
    Xml,
    Text,
    Html,
    JavaScript,
}

impl RawKind {
    pub const ALL: [RawKind; 5] = [RawKind::Json, RawKind::Xml, RawKind::Text, RawKind::Html, RawKind::JavaScript];

    pub fn label(self) -> &'static str {
        match self {
            RawKind::Json => "JSON",
            RawKind::Xml => "XML",
            RawKind::Text => "Text",
            RawKind::Html => "HTML",
            RawKind::JavaScript => "JavaScript",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            RawKind::Json => "application/json",
            RawKind::Xml => "application/xml",
            RawKind::Text => "text/plain",
            RawKind::Html => "text/html",
            RawKind::JavaScript => "application/javascript",
        }
    }
}

/// Тело запроса. form-data и binary появятся на этапе v1.0 (FR-05).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Body {
    #[default]
    None,
    Raw {
        text: String,
        kind: RawKind,
    },
    UrlEncoded(Vec<KeyValue>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

/// Авторизация запроса. Здесь виды этапа MVP; Digest, OAuth 2.0, JWT,
/// AWS SigV4, OAuth 1.0a и NTLM добавляются по плану из раздела 5.1.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    #[default]
    Inherit,
    None,
    ApiKey {
        key: String,
        value: String,
        location: ApiKeyLocation,
    },
    /// Пустой `prefix` означает "Bearer".
    Bearer {
        token: String,
        prefix: String,
    },
    Basic {
        username: String,
        password: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestSettings {
    /// `None` — значение по умолчанию (30 с).
    pub timeout_ms: Option<u64>,
    pub follow_redirects: bool,
    pub max_redirects: u8,
    pub verify_tls: bool,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self { timeout_ms: None, follow_redirects: true, max_redirects: 10, verify_tls: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub id: Uuid,
    pub name: String,
    pub method: Method,
    /// Сырой текст URL вместе с query и `{{переменными}}`.
    pub url: String,
    /// Таблица Params; синхронизируется с `url` (см. `url_sync`).
    pub query: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
    pub settings: RequestSettings,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Новый запрос".to_owned(),
            method: Method::Get,
            url: String::new(),
            query: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: RequestSettings::default(),
        }
    }
}

impl Request {
    /// Копия без пустых строк-заготовок — для сохранения на диск.
    pub fn without_blank_rows(&self) -> Request {
        let mut r = self.clone();
        r.query.retain(|kv| !kv.is_blank());
        r.headers.retain(|kv| !kv.is_blank());
        if let Body::UrlEncoded(fields) = &mut r.body {
            fields.retain(|kv| !kv.is_blank());
        }
        r
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Variable {
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub secret: bool,
}

impl Default for Variable {
    fn default() -> Self {
        Self { key: String::new(), value: String::new(), enabled: true, secret: false }
    }
}

impl Variable {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), ..Self::default() }
    }

    pub fn is_blank(&self) -> bool {
        self.key.trim().is_empty() && self.value.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    pub id: Uuid,
    pub name: String,
    pub variables: Vec<Variable>,
}

impl Default for Environment {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "Новое окружение".to_owned(), variables: Vec::new() }
    }
}

/// Состояние рабочего пространства, которое переживает перезапуск (`workspace.json`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    /// Открытые вкладки вместе с несохранёнными правками (черновики).
    pub tabs: Vec<Request>,
    pub active_tab: usize,
    pub active_env: Option<Uuid>,
}

/// Всё, что сохраняется на диск за один раз.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkspaceSnapshot {
    pub workspace: Workspace,
    pub environments: Vec<Environment>,
    pub globals: Vec<Variable>,
    pub collections: Vec<crate::collection::Collection>,
    /// Новые записи первыми.
    pub history: Vec<crate::collection::HistoryEntry>,
}

/// Полученный ответ. Тело в историю не пишется.
#[derive(Debug, Clone)]
pub struct ResponseData {
    pub status: u16,
    pub reason: String,
    pub http_version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub size_bytes: u64,
    pub elapsed_ms: u64,
    pub ttfb_ms: u64,
    pub final_url: String,
}

impl ResponseData {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }

    pub fn is_json(&self) -> bool {
        self.content_type().is_some_and(|ct| ct.to_ascii_lowercase().contains("json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_serializes_as_uppercase_and_custom_as_is() {
        assert_eq!(serde_json::to_string(&Method::Patch).unwrap(), "\"PATCH\"");
        let custom: Method = serde_json::from_str("\"PURGE\"").unwrap();
        assert_eq!(custom, Method::Custom("PURGE".into()));
        let get: Method = serde_json::from_str("\"GET\"").unwrap();
        assert_eq!(get, Method::Get);
    }

    #[test]
    fn request_roundtrips_through_json() {
        let r = Request {
            url: "{{baseUrl}}/users".into(),
            body: Body::UrlEncoded(vec![KeyValue::new("a", "1")]),
            auth: Auth::Bearer { token: "{{token}}".into(), prefix: String::new() },
            ..Default::default()
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn blank_rows_are_dropped() {
        let r = Request { headers: vec![KeyValue::new("Accept", "*/*"), KeyValue::default()], ..Default::default() };
        assert_eq!(r.without_blank_rows().headers.len(), 1);
    }
}
