//! Импорт и экспорт: экспорт в cURL (FR-60), импорт команд cURL (FR-61),
//! коллекций и окружений Postman (FR-62).

pub mod curl;
pub mod postman;

use queryman_core::collection::Collection;
use queryman_core::model::{Environment, Request};

/// Ошибка разбора импортируемого текста, понятная пользователю.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ImportError(String);

impl ImportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Результат импорта.
#[derive(Debug, Clone, PartialEq)]
pub enum Imported {
    Request(Request),
    Collection { collection: Collection, environment: Option<Environment> },
    Environment(Environment),
}

/// Определяет формат по содержимому: команда cURL, коллекция или окружение Postman.
pub fn import(text: &str) -> Result<Imported, ImportError> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err(ImportError::new("нечего импортировать"));
    }
    if curl::looks_like_curl(text) {
        return curl::from_curl(text).map(Imported::Request);
    }
    if text.starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| ImportError::new(format!("некорректный JSON: {e}")))?;
        if postman::is_collection(&value) {
            let (collection, environment) = postman::collection_from_value(&value)?;
            return Ok(Imported::Collection { collection, environment });
        }
        if postman::is_environment(&value) {
            return postman::environment_from_value(&value).map(Imported::Environment);
        }
        return Err(ImportError::new("JSON не похож на коллекцию или окружение Postman"));
    }
    Err(ImportError::new("ожидается команда cURL или JSON коллекции Postman"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_format() {
        assert!(matches!(import("  curl http://x"), Ok(Imported::Request(_))));
        assert!(matches!(
            import(r#"{"info": {"name": "a"}, "item": []}"#),
            Ok(Imported::Collection { environment: None, .. })
        ));
        assert!(matches!(import(r#"{"name": "e", "values": []}"#), Ok(Imported::Environment(_))));
        assert!(import(r#"{"a": 1}"#).is_err());
        assert!(import("hello").is_err());
        assert!(import("").is_err());
    }
}
