//! Импорт коллекций Postman v2.0/v2.1 и окружений Postman (FR-62).
//!
//! Неизвестные поля пропускаются. Скрипты коллекции и папок добавляются
//! в начало скриптов каждого запроса: у Queryman скрипты есть только у запросов.

use crate::ImportError;
use queryman_core::collection::{Collection, Folder, Item};
use queryman_core::model::{
    ApiKeyLocation, Auth, Body, Environment, FormField, KeyValue, Method, RawKind, Request, Variable,
};
use queryman_core::script::Scripts;
use queryman_core::url_sync;
use serde_json::Value;

/// Похоже ли на коллекцию Postman.
pub fn is_collection(v: &Value) -> bool {
    v.get("info").and_then(|i| i.get("schema")).and_then(Value::as_str).is_some_and(|s| s.contains("getpostman.com"))
        || (v.get("info").is_some() && v.get("item").is_some_and(Value::is_array))
}

/// Похоже ли на окружение Postman.
pub fn is_environment(v: &Value) -> bool {
    v.get("values").is_some_and(Value::is_array) && v.get("name").is_some_and(Value::is_string)
}

/// Коллекция и окружение из её переменных (если они есть).
pub fn collection_from_value(v: &Value) -> Result<(Collection, Option<Environment>), ImportError> {
    if !is_collection(v) {
        return Err(ImportError::new("это не коллекция Postman"));
    }
    let name = str_at(v, &["info", "name"]).unwrap_or("Импорт из Postman").to_owned();
    let auth = v.get("auth").map(parse_auth).unwrap_or(Auth::None);
    let scripts = parse_events(v.get("event"));
    let items = v.get("item").and_then(Value::as_array).map(|a| parse_items(a, &scripts)).unwrap_or_default();
    let variables: Vec<Variable> = v
        .get("variable")
        .and_then(Value::as_array)
        .map(|vars| vars.iter().filter_map(parse_variable).collect())
        .unwrap_or_default();
    let environment = (!variables.is_empty()).then(|| Environment {
        name: format!("{name} (переменные)"),
        variables,
        ..Default::default()
    });
    // У коллекции «Наследовать» значит «без авторизации».
    let auth = if auth == Auth::Inherit { Auth::None } else { auth };
    Ok((Collection { name, auth, items, ..Default::default() }, environment))
}

pub fn environment_from_value(v: &Value) -> Result<Environment, ImportError> {
    if !is_environment(v) {
        return Err(ImportError::new("это не окружение Postman"));
    }
    Ok(Environment {
        name: v.get("name").and_then(Value::as_str).unwrap_or("Окружение").to_owned(),
        variables: v["values"].as_array().map(|a| a.iter().filter_map(parse_variable).collect()).unwrap_or_default(),
        ..Default::default()
    })
}

fn parse_items(items: &[Value], inherited: &Scripts) -> Vec<Item> {
    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
            if let Some(children) = item.get("item").and_then(Value::as_array) {
                let scripts = chain(inherited, &parse_events(item.get("event")));
                Some(Item::Folder(Folder {
                    name: if name.is_empty() { "Папка".into() } else { name },
                    auth: item.get("auth").map(parse_auth).unwrap_or(Auth::Inherit),
                    items: parse_items(children, &scripts),
                    ..Default::default()
                }))
            } else {
                let request = item.get("request")?;
                let mut r = parse_request(request);
                if !name.is_empty() {
                    r.name = name;
                }
                r.scripts = chain(inherited, &parse_events(item.get("event")));
                Some(Item::Request(r))
            }
        })
        .collect()
}

fn parse_request(v: &Value) -> Request {
    // Запрос может быть просто строкой URL.
    if let Some(url) = v.as_str() {
        return Request {
            name: crate::curl::request_name(&Method::Get, url),
            query: url_sync::params_from_url(url, &[]),
            url: url.to_owned(),
            ..Default::default()
        };
    }
    let method_text = v.get("method").and_then(Value::as_str).unwrap_or("GET").to_uppercase();
    let method =
        Method::STANDARD.iter().find(|m| m.as_str() == method_text).cloned().unwrap_or(Method::Custom(method_text));
    let (url, disabled_query) = parse_url(v.get("url"));
    let mut query = url_sync::params_from_url(&url, &[]);
    query.extend(disabled_query);
    let headers =
        v.get("header").and_then(Value::as_array).map(|h| h.iter().filter_map(parse_kv).collect()).unwrap_or_default();
    Request {
        name: crate::curl::request_name(&method, &url),
        method,
        url,
        query,
        headers,
        body: v.get("body").map(parse_body).unwrap_or_default(),
        auth: v.get("auth").map(parse_auth).unwrap_or(Auth::Inherit),
        ..Default::default()
    }
}

/// Текст URL и выключенные параметры (в `raw` их нет).
fn parse_url(v: Option<&Value>) -> (String, Vec<KeyValue>) {
    match v {
        None => (String::new(), Vec::new()),
        Some(Value::String(s)) => (s.clone(), Vec::new()),
        Some(obj) => {
            let raw = obj.get("raw").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| build_url(obj));
            let disabled = obj
                .get("query")
                .and_then(Value::as_array)
                .map(|q| q.iter().filter_map(parse_kv).filter(|kv| !kv.enabled).collect())
                .unwrap_or_default();
            (raw, disabled)
        }
    }
}

/// URL из частей, когда `raw` нет.
fn build_url(obj: &Value) -> String {
    let join = |key: &str, sep: &str| match obj.get(key) {
        Some(Value::Array(parts)) => parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(sep),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let mut url = String::new();
    if let Some(protocol) = obj.get("protocol").and_then(Value::as_str) {
        url.push_str(protocol);
        url.push_str("://");
    }
    url.push_str(&join("host", "."));
    if let Some(port) = obj.get("port").and_then(Value::as_str) {
        url.push(':');
        url.push_str(port);
    }
    let path = join("path", "/");
    if !path.is_empty() {
        url.push('/');
        url.push_str(path.trim_start_matches('/'));
    }
    let query: Vec<KeyValue> = obj
        .get("query")
        .and_then(Value::as_array)
        .map(|q| q.iter().filter_map(parse_kv).filter(|kv| kv.enabled).collect())
        .unwrap_or_default();
    if !query.is_empty() {
        url = url_sync::url_from_params(&url, &query);
    }
    url
}

fn parse_kv(v: &Value) -> Option<KeyValue> {
    let key = v.get("key").and_then(Value::as_str)?;
    Some(KeyValue {
        key: key.to_owned(),
        value: value_string(v.get("value")),
        enabled: !v.get("disabled").and_then(Value::as_bool).unwrap_or(false),
        description: description(v.get("description")),
    })
}

fn parse_variable(v: &Value) -> Option<Variable> {
    let key = v.get("key").and_then(Value::as_str)?;
    Some(Variable {
        key: key.to_owned(),
        value: value_string(v.get("value")),
        enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(true)
            && !v.get("disabled").and_then(Value::as_bool).unwrap_or(false),
        secret: v.get("type").and_then(Value::as_str) == Some("secret"),
    })
}

fn parse_body(v: &Value) -> Body {
    if v.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
        return Body::None;
    }
    match v.get("mode").and_then(Value::as_str) {
        Some("raw") => {
            let text = value_string(v.get("raw"));
            let kind = match str_at(v, &["options", "raw", "language"]) {
                Some("json") => RawKind::Json,
                Some("xml") => RawKind::Xml,
                Some("html") => RawKind::Html,
                Some("javascript") => RawKind::JavaScript,
                Some(_) => RawKind::Text,
                None if serde_json::from_str::<Value>(&text).is_ok() => RawKind::Json,
                None => RawKind::Text,
            };
            Body::Raw { text, kind }
        }
        Some("urlencoded") => Body::UrlEncoded(
            v.get("urlencoded")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(parse_kv).collect())
                .unwrap_or_default(),
        ),
        Some("formdata") => Body::FormData(
            v.get("formdata")
                .and_then(Value::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(|f| {
                            let kv = parse_kv(f)?;
                            let file = f.get("type").and_then(Value::as_str) == Some("file");
                            let value = if file { file_src(f.get("src")) } else { kv.value };
                            Some(FormField {
                                key: kv.key,
                                value,
                                file,
                                enabled: kv.enabled,
                                description: kv.description,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some("file") => Body::Binary { path: file_src(v.get("file").and_then(|f| f.get("src"))) },
        Some("graphql") => {
            let query = str_at(v, &["graphql", "query"]).unwrap_or_default();
            let variables = match v.get("graphql").and_then(|g| g.get("variables")) {
                Some(Value::String(s)) if !s.trim().is_empty() => {
                    serde_json::from_str(s).unwrap_or(Value::String(s.clone()))
                }
                Some(Value::String(_)) | None => Value::Object(Default::default()),
                Some(other) => other.clone(),
            };
            let payload = serde_json::json!({ "query": query, "variables": variables });
            Body::Raw { text: serde_json::to_string_pretty(&payload).unwrap_or_default(), kind: RawKind::Json }
        }
        _ => Body::None,
    }
}

fn file_src(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.first().and_then(Value::as_str).unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

fn parse_auth(v: &Value) -> Auth {
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("noauth");
    // В v2.1 параметры — массив {key, value}; в v2.0 — объект.
    let param = |name: &str| -> String {
        match v.get(kind) {
            Some(Value::Array(params)) => params
                .iter()
                .find(|p| p.get("key").and_then(Value::as_str) == Some(name))
                .map(|p| value_string(p.get("value")))
                .unwrap_or_default(),
            Some(Value::Object(map)) => value_string(map.get(name)),
            _ => String::new(),
        }
    };
    match kind {
        "noauth" => Auth::None,
        "bearer" => Auth::Bearer { token: param("token"), prefix: String::new() },
        "basic" => Auth::Basic { username: param("username"), password: param("password") },
        "apikey" => Auth::ApiKey {
            key: param("key"),
            value: param("value"),
            location: if param("in") == "query" { ApiKeyLocation::Query } else { ApiKeyLocation::Header },
        },
        // Остальные виды пока не поддерживаются: запрос наследует авторизацию.
        _ => Auth::Inherit,
    }
}

fn parse_events(v: Option<&Value>) -> Scripts {
    let mut scripts = Scripts::default();
    for event in v.and_then(Value::as_array).into_iter().flatten() {
        if event.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let code = match event.get("script").and_then(|s| s.get("exec")) {
            Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
            Some(Value::String(s)) => s.clone(),
            _ => continue,
        };
        if code.trim().is_empty() {
            continue;
        }
        match event.get("listen").and_then(Value::as_str) {
            Some("prerequest") => append(&mut scripts.pre_request, &code),
            Some("test") => append(&mut scripts.test, &code),
            _ => {}
        }
    }
    scripts
}

fn chain(parent: &Scripts, own: &Scripts) -> Scripts {
    let mut out = parent.clone();
    append(&mut out.pre_request, &own.pre_request);
    append(&mut out.test, &own.test);
    out
}

fn append(target: &mut String, code: &str) {
    if code.trim().is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push_str("\n\n");
    }
    target.push_str(code);
}

fn value_string(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn description(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(obj) => obj.get("content").and_then(Value::as_str).unwrap_or_default().to_owned(),
        None => String::new(),
    }
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, key| v.get(key))?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &str = r#"{
      "info": { "name": "Shop API", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "{{token}}", "type": "string" }] },
      "event": [{ "listen": "prerequest", "script": { "exec": ["pm.environment.set('a', 1);"] } }],
      "variable": [{ "key": "baseUrl", "value": "https://shop.test" }],
      "item": [
        {
          "name": "Users",
          "auth": { "type": "basic", "basic": [{ "key": "username", "value": "u" }, { "key": "password", "value": "p" }] },
          "item": [
            {
              "name": "List users",
              "event": [{ "listen": "test", "script": { "exec": ["pm.test('ok', () => pm.response.to.be.ok);"] } }],
              "request": {
                "method": "GET",
                "header": [{ "key": "Accept", "value": "application/json" }, { "key": "X-Off", "value": "1", "disabled": true }],
                "url": { "raw": "{{baseUrl}}/users?page=1", "query": [{ "key": "page", "value": "1" }, { "key": "debug", "value": "1", "disabled": true }] }
              }
            },
            {
              "name": "Upload avatar",
              "request": {
                "method": "POST",
                "body": { "mode": "formdata", "formdata": [
                  { "key": "avatar", "type": "file", "src": "/home/me/a.png" },
                  { "key": "note", "value": "hi", "type": "text" }
                ] },
                "url": "{{baseUrl}}/avatar"
              }
            }
          ]
        },
        {
          "name": "Create order",
          "request": {
            "method": "POST",
            "auth": { "type": "apikey", "apikey": [{ "key": "key", "value": "X-Key" }, { "key": "value", "value": "k" }, { "key": "in", "value": "query" }] },
            "body": { "mode": "raw", "raw": "{\"id\": 1}", "options": { "raw": { "language": "json" } } },
            "url": { "protocol": "https", "host": ["shop", "test"], "path": ["orders"] }
          }
        },
        { "name": "Login", "request": { "method": "POST", "url": "{{baseUrl}}/login",
          "body": { "mode": "urlencoded", "urlencoded": [{ "key": "user", "value": "a" }] } } },
        { "name": "Graph", "request": { "method": "POST", "url": "{{baseUrl}}/graphql",
          "body": { "mode": "graphql", "graphql": { "query": "{ me { id } }", "variables": "" } } } }
      ]
    }"#;

    #[test]
    fn imports_collection_tree() {
        let v: Value = serde_json::from_str(COLLECTION).unwrap();
        let (c, env) = collection_from_value(&v).unwrap();
        assert_eq!(c.name, "Shop API");
        assert_eq!(c.auth, Auth::Bearer { token: "{{token}}".into(), prefix: String::new() });
        let env = env.unwrap();
        assert_eq!(env.name, "Shop API (переменные)");
        assert_eq!(env.variables, vec![Variable::new("baseUrl", "https://shop.test")]);

        let Item::Folder(users) = &c.items[0] else { panic!("ожидалась папка") };
        assert_eq!(users.auth, Auth::Basic { username: "u".into(), password: "p".into() });
        let Item::Request(list) = &users.items[0] else { panic!() };
        assert_eq!(list.name, "List users");
        assert_eq!(list.url, "{{baseUrl}}/users?page=1");
        assert_eq!(list.query.len(), 2);
        assert!(!list.query[1].enabled);
        assert!(!list.headers[1].enabled);
        assert_eq!(list.auth, Auth::Inherit);
        assert_eq!(list.scripts.pre_request, "pm.environment.set('a', 1);");
        assert!(list.scripts.test.contains("pm.response.to.be.ok"));

        let Item::Request(upload) = &users.items[1] else { panic!() };
        assert_eq!(
            upload.body,
            Body::FormData(vec![FormField::file("avatar", "/home/me/a.png"), FormField::text("note", "hi")])
        );

        let Item::Request(order) = &c.items[1] else { panic!() };
        assert_eq!(order.url, "https://shop.test/orders");
        assert_eq!(order.body, Body::Raw { text: "{\"id\": 1}".into(), kind: RawKind::Json });
        assert!(matches!(order.auth, Auth::ApiKey { location: ApiKeyLocation::Query, .. }));

        let Item::Request(login) = &c.items[2] else { panic!() };
        assert_eq!(login.body, Body::UrlEncoded(vec![KeyValue::new("user", "a")]));

        let Item::Request(graph) = &c.items[3] else { panic!() };
        let Body::Raw { text, .. } = &graph.body else { panic!() };
        assert!(text.contains("\"query\": \"{ me { id } }\""));
    }

    #[test]
    fn imports_environment() {
        let v: Value = serde_json::from_str(
            r#"{ "name": "Prod", "values": [
                { "key": "host", "value": "x", "enabled": true },
                { "key": "token", "value": "s", "type": "secret", "enabled": false }
            ], "_postman_variable_scope": "environment" }"#,
        )
        .unwrap();
        let env = environment_from_value(&v).unwrap();
        assert_eq!(env.name, "Prod");
        assert_eq!(env.variables[0], Variable::new("host", "x"));
        assert!(env.variables[1].secret && !env.variables[1].enabled);
        assert!(collection_from_value(&v).is_err());
    }
}
