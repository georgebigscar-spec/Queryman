//! Подготовка запроса к отправке: подстановка переменных, тело, авторизация.
//! Выполняется в UI-потоке перед отправкой команды (раздел 3).

use crate::model::{ApiKeyLocation, Auth, Body, Request, RequestSettings};
use crate::vars::Resolver;
use base64::Engine as _;
use url::form_urlencoded;

/// Запрос с подставленными значениями — то, что реально уходит в сеть.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub settings: RequestSettings,
}

impl ResolvedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// `parent_auth` — авторизация папки или коллекции для режима Inherit.
pub fn prepare(req: &Request, resolver: &Resolver<'_>, parent_auth: Option<&Auth>) -> ResolvedRequest {
    let mut url = resolver.resolve(req.url.trim());

    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.trim().is_empty())
        .map(|h| (resolver.resolve(h.key.trim()), resolver.resolve(&h.value)))
        .collect();

    let body = match &req.body {
        Body::None => None,
        Body::Raw { text, .. } if text.is_empty() => None,
        Body::Raw { text, kind } => {
            set_default_header(&mut headers, "Content-Type", kind.content_type());
            Some(resolver.resolve(text).into_bytes())
        }
        Body::UrlEncoded(fields) => {
            let mut ser = form_urlencoded::Serializer::new(String::new());
            for f in fields.iter().filter(|f| f.enabled && !f.key.is_empty()) {
                ser.append_pair(&resolver.resolve(&f.key), &resolver.resolve(&f.value));
            }
            set_default_header(&mut headers, "Content-Type", "application/x-www-form-urlencoded");
            Some(ser.finish().into_bytes())
        }
    };

    let auth = match &req.auth {
        Auth::Inherit => parent_auth.unwrap_or(&Auth::None),
        own => own,
    };
    apply_auth(auth, resolver, &mut url, &mut headers);

    ResolvedRequest {
        method: req.method.as_str().to_owned(),
        url: with_default_scheme(url),
        headers,
        body,
        settings: req.settings.clone(),
    }
}

fn apply_auth(auth: &Auth, r: &Resolver<'_>, url: &mut String, headers: &mut Vec<(String, String)>) {
    match auth {
        Auth::Inherit | Auth::None => {}
        Auth::ApiKey { key, value, location } => {
            let key = r.resolve(key);
            if key.trim().is_empty() {
                return;
            }
            let value = r.resolve(value);
            match location {
                ApiKeyLocation::Header => set_default_header(headers, &key, &value),
                ApiKeyLocation::Query => {
                    url.push(if url.contains('?') { '&' } else { '?' });
                    url.extend(form_urlencoded::byte_serialize(key.as_bytes()));
                    url.push('=');
                    url.extend(form_urlencoded::byte_serialize(value.as_bytes()));
                }
            }
        }
        Auth::Bearer { token, prefix } => {
            let token = r.resolve(token);
            if token.is_empty() {
                return;
            }
            let prefix = if prefix.trim().is_empty() { "Bearer".to_owned() } else { r.resolve(prefix.trim()) };
            set_default_header(headers, "Authorization", &format!("{prefix} {token}"));
        }
        Auth::Basic { username, password } => {
            let raw = format!("{}:{}", r.resolve(username), r.resolve(password));
            let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
            set_default_header(headers, "Authorization", &format!("Basic {encoded}"));
        }
    }
}

/// Добавляет заголовок, только если пользователь не задал его сам
/// (правило 3 раздела 5.1: ручной заголовок важнее вкладки Auth).
fn set_default_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) {
        headers.push((name.to_owned(), value.to_owned()));
    }
}

fn with_default_scheme(url: String) -> String {
    if url.is_empty() || url.contains("://") {
        url
    } else {
        format!("http://{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyValue, RawKind, Variable};

    fn request(url: &str) -> Request {
        Request { url: url.into(), ..Request::default() }
    }

    #[test]
    fn variables_headers_and_scheme() {
        let vars = [Variable::new("host", "localhost:3000"), Variable::new("v", "2")];
        let r = Resolver::new().with_layer(&vars);
        let mut req = request("{{host}}/api");
        let mut off = KeyValue::new("X-Off", "1");
        off.enabled = false;
        req.headers = vec![KeyValue::new("X-Version", "{{v}}"), off, KeyValue::default()];
        let out = prepare(&req, &r, None);
        assert_eq!(out.url, "http://localhost:3000/api");
        assert_eq!(out.headers, vec![("X-Version".to_owned(), "2".to_owned())]);
        assert_eq!(out.body, None);
    }

    #[test]
    fn raw_json_sets_content_type() {
        let vars = [Variable::new("id", "7")];
        let mut req = request("http://x");
        req.body = Body::Raw { text: r#"{"id": {{id}}}"#.into(), kind: RawKind::Json };
        let out = prepare(&req, &Resolver::new().with_layer(&vars), None);
        assert_eq!(out.header("content-type"), Some("application/json"));
        assert_eq!(out.body.as_deref(), Some(br#"{"id": 7}"#.as_slice()));
    }

    #[test]
    fn urlencoded_body_is_encoded() {
        let mut req = request("http://x");
        req.body = Body::UrlEncoded(vec![KeyValue::new("a", "1 2"), KeyValue::new("b", "&")]);
        let out = prepare(&req, &Resolver::new(), None);
        assert_eq!(out.body.as_deref(), Some(b"a=1+2&b=%26".as_slice()));
    }

    #[test]
    fn basic_and_bearer() {
        let mut req = request("http://x");
        req.auth = Auth::Basic { username: "user".into(), password: "pass".into() };
        assert_eq!(prepare(&req, &Resolver::new(), None).header("authorization"), Some("Basic dXNlcjpwYXNz"));
        req.auth = Auth::Bearer { token: "abc".into(), prefix: String::new() };
        assert_eq!(prepare(&req, &Resolver::new(), None).header("authorization"), Some("Bearer abc"));
    }

    #[test]
    fn api_key_in_query_and_inheritance() {
        let parent = Auth::ApiKey { key: "api key".into(), value: "a&b".into(), location: ApiKeyLocation::Query };
        let out = prepare(&request("http://x/p?q=1"), &Resolver::new(), Some(&parent));
        assert_eq!(out.url, "http://x/p?q=1&api+key=a%26b");
    }

    #[test]
    fn manual_authorization_header_wins() {
        let mut req = request("http://x");
        req.headers = vec![KeyValue::new("authorization", "Custom 1")];
        req.auth = Auth::Bearer { token: "abc".into(), prefix: String::new() };
        let out = prepare(&req, &Resolver::new(), None);
        assert_eq!(out.headers.len(), 1);
        assert_eq!(out.header("Authorization"), Some("Custom 1"));
    }
}
