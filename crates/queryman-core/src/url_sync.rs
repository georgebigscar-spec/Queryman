//! Двусторонняя синхронизация поля URL и таблицы Params (FR-02).
//!
//! Работает с сырым текстом без декодирования, чтобы не ломать `{{переменные}}`
//! и уже закодированные значения. Кодирование выполняет `url::Url` при отправке.

use crate::model::KeyValue;

/// Делит URL на базу и строку query (без `?`).
pub fn split_url(url: &str) -> (&str, Option<&str>) {
    match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    }
}

pub fn parse_query(query: &str) -> Vec<KeyValue> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => KeyValue::new(k, v),
            None => KeyValue::new(pair, ""),
        })
        .collect()
}

/// URL изменён пользователем → новая таблица Params.
/// Выключенные строки из прежней таблицы сохраняются (в URL их нет).
pub fn params_from_url(url: &str, previous: &[KeyValue]) -> Vec<KeyValue> {
    let mut params = split_url(url).1.map(parse_query).unwrap_or_default();
    params.extend(previous.iter().filter(|p| !p.enabled && !p.is_blank()).cloned());
    params
}

/// Таблица Params изменена → новый текст URL.
pub fn url_from_params(url: &str, params: &[KeyValue]) -> String {
    let base = split_url(url).0;
    let pairs: Vec<String> = params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .map(|p| if p.value.is_empty() { p.key.clone() } else { format!("{}={}", p.key, p.value) })
        .collect();
    if pairs.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{}", pairs.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_to_params_and_back() {
        let url = "{{base}}/users?page=1&q={{term}}&flag";
        let params = params_from_url(url, &[]);
        assert_eq!(params.len(), 3);
        assert_eq!(params[1].value, "{{term}}");
        assert_eq!(url_from_params(url, &params), url);
    }

    #[test]
    fn disabled_params_survive_url_edit() {
        let mut off = KeyValue::new("debug", "1");
        off.enabled = false;
        let params = params_from_url("http://x/a?b=2", &[KeyValue::new("old", "1"), off]);
        assert_eq!(params.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["b", "debug"]);
        assert_eq!(url_from_params("http://x/a?b=2", &params), "http://x/a?b=2");
    }

    #[test]
    fn empty_params_remove_question_mark() {
        assert_eq!(url_from_params("http://x/a?b=2", &[]), "http://x/a");
    }
}
