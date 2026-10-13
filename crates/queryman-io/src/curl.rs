//! cURL: экспорт подготовленного запроса (FR-60) и импорт команды (FR-61).

use crate::ImportError;
use queryman_core::model::{Auth, Body, FormField, KeyValue, Method, RawKind, Request, RequestSettings};
use queryman_core::prepare::{PartValue, ResolvedBody, ResolvedRequest};
use queryman_core::url_sync;

/// Команда cURL, которая выполняет тот же запрос.
/// Значения авторизации уже вычислены — команда может содержать секреты.
pub fn to_curl(req: &ResolvedRequest) -> String {
    let mut parts = vec!["curl".to_owned()];
    if req.method != "GET" || req.body.is_some() {
        parts.push(format!("-X {}", req.method));
    }
    parts.push(quote(&req.url));
    let multipart = matches!(req.body, Some(ResolvedBody::Multipart(_)));
    for (name, value) in &req.headers {
        // Для -F curl сам ставит Content-Type с границей.
        if multipart && name.eq_ignore_ascii_case("content-type") && value.starts_with("multipart/form-data") {
            continue;
        }
        parts.push(format!("-H {}", quote(&format!("{name}: {value}"))));
    }
    match &req.body {
        None => {}
        Some(ResolvedBody::Bytes(body)) => parts.push(format!("--data-raw {}", quote(&String::from_utf8_lossy(body)))),
        Some(ResolvedBody::File(path)) => {
            parts.push(format!("--data-binary {}", quote(&format!("@{}", path.display()))))
        }
        Some(ResolvedBody::Multipart(fields)) => {
            for field in fields {
                let value = match &field.value {
                    PartValue::Text(text) => format!("{}={text}", field.name),
                    PartValue::File(path) => format!("{}=@{}", field.name, path.display()),
                };
                parts.push(format!("-F {}", quote(&value)));
            }
        }
    }
    if req.settings.follow_redirects {
        parts.push(format!("-L --max-redirs {}", req.settings.max_redirects));
    }
    if !req.settings.verify_tls {
        parts.push("--insecure".to_owned());
    }
    if let Some(ms) = req.settings.timeout_ms {
        parts.push(format!("--max-time {}", (ms as f64 / 1000.0)));
    }
    parts.join(" \\\n  ")
}

/// Одинарные кавычки POSIX-shell: `it's` → `'it'\''s'`.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Похоже ли на команду cURL (для вставки в поле URL).
pub fn looks_like_curl(text: &str) -> bool {
    let t = text.trim_start();
    t.strip_prefix("curl").is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// Флаги cURL с аргументом, которые не влияют на запрос и пропускаются.
const IGNORED_WITH_VALUE: &[&str] = &[
    "-o",
    "--output",
    "-w",
    "--write-out",
    "--connect-timeout",
    "-x",
    "--proxy",
    "-U",
    "--proxy-user",
    "--retry",
    "-c",
    "--cookie-jar",
    "-r",
    "--range",
    "--cacert",
    "--cert",
    "--key",
    "-T",
    "--upload-file",
    "--resolve",
    "--limit-rate",
];

/// Разбирает команду cURL (bash или cmd с `^`) в запрос.
pub fn from_curl(command: &str) -> Result<Request, ImportError> {
    let args = split_args(command)?;
    let mut args = args.into_iter().peekable();
    match args.next() {
        Some(first) if first == "curl" || first.ends_with("/curl") || first.eq_ignore_ascii_case("curl.exe") => {}
        _ => return Err(ImportError::new("команда должна начинаться с curl")),
    }

    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<KeyValue> = Vec::new();
    let mut data: Vec<String> = Vec::new();
    let mut urlencode: Vec<String> = Vec::new();
    let mut binary_file: Option<String> = None;
    let mut form: Vec<FormField> = Vec::new();
    let mut auth = Auth::Inherit;
    let mut settings = RequestSettings { follow_redirects: false, ..Default::default() };
    let mut get = false;

    while let Some(arg) = args.next() {
        // --flag=value
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        // -XPOST, -HAccept:… — короткий флаг со слитным значением.
        let (flag, inline) = if inline.is_none() && flag.len() > 2 && flag.starts_with('-') && !flag.starts_with("--") {
            let short = &flag[..2];
            if matches!(short, "-X" | "-H" | "-d" | "-F" | "-u" | "-A" | "-b" | "-e" | "-m") {
                (short.to_owned(), Some(flag[2..].to_owned()))
            } else {
                (flag, inline)
            }
        } else {
            (flag, inline)
        };
        let mut value = |name: &str| -> Result<String, ImportError> {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| ImportError::new(format!("у флага {name} нет значения")))
        };

        match flag.as_str() {
            "-X" | "--request" => method = Some(value(&flag)?.to_uppercase()),
            "-H" | "--header" => {
                let raw = value(&flag)?;
                if let Some((k, v)) = raw.split_once(':') {
                    headers.push(KeyValue::new(k.trim(), v.trim()));
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-ascii" => {
                let v = value(&flag)?;
                match v.strip_prefix('@').filter(|_| flag != "--data-raw") {
                    Some(path) => binary_file = Some(path.to_owned()),
                    None => data.push(v),
                }
            }
            "--data-binary" => {
                let v = value(&flag)?;
                match v.strip_prefix('@') {
                    Some(path) => binary_file = Some(path.to_owned()),
                    None => data.push(v),
                }
            }
            "--data-urlencode" => urlencode.push(value(&flag)?),
            "--json" => {
                data.push(value(&flag)?);
                if !headers.iter().any(|h| h.key.eq_ignore_ascii_case("content-type")) {
                    headers.push(KeyValue::new("Content-Type", "application/json"));
                }
            }
            "-F" | "--form" | "--form-string" => {
                let v = value(&flag)?;
                let (k, val) = v.split_once('=').unwrap_or((v.as_str(), ""));
                let field = match val.strip_prefix('@').filter(|_| flag != "--form-string") {
                    // `@file;type=image/png` — тип файла определит исполнитель.
                    Some(path) => FormField::file(k, path.split(";type=").next().unwrap_or(path)),
                    None => FormField::text(k, val),
                };
                form.push(field);
            }
            "-u" | "--user" => {
                let v = value(&flag)?;
                let (username, password) = v.split_once(':').unwrap_or((v.as_str(), ""));
                auth = Auth::Basic { username: username.to_owned(), password: password.to_owned() };
            }
            "-A" | "--user-agent" => headers.push(KeyValue::new("User-Agent", value(&flag)?)),
            "-b" | "--cookie" => headers.push(KeyValue::new("Cookie", value(&flag)?)),
            "-e" | "--referer" => headers.push(KeyValue::new("Referer", value(&flag)?)),
            "-m" | "--max-time" => {
                let v = value(&flag)?;
                if let Ok(secs) = v.parse::<f64>() {
                    settings.timeout_ms = Some((secs * 1000.0).round() as u64);
                }
            }
            "--max-redirs" => {
                if let Ok(n) = value(&flag)?.parse::<u8>() {
                    settings.max_redirects = n;
                }
            }
            "--url" => url = Some(value(&flag)?),
            "-L" | "--location" => settings.follow_redirects = true,
            "-k" | "--insecure" => settings.verify_tls = false,
            "-G" | "--get" => get = true,
            "-I" | "--head" => method = Some("HEAD".to_owned()),
            f if IGNORED_WITH_VALUE.contains(&f) => {
                if inline.is_none() {
                    args.next();
                }
            }
            f if f.starts_with('-') && f.len() > 1 => {} // -s, -v, --compressed и прочие без значения
            _ => {
                if url.is_none() {
                    url = Some(arg);
                }
            }
        }
    }

    let mut url = url.ok_or_else(|| ImportError::new("в команде нет адреса"))?;
    let content_type =
        headers.iter().find(|h| h.key.eq_ignore_ascii_case("content-type")).map(|h| h.value.to_ascii_lowercase());

    let mut body = Body::None;
    if get {
        let mut pairs = data.clone();
        pairs.extend(urlencode.iter().map(|p| encode_pair(p)));
        if !pairs.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&pairs.join("&"));
        }
    } else if !form.is_empty() {
        body = Body::FormData(form);
    } else if let Some(path) = binary_file {
        body = Body::Binary { path };
    } else if !urlencode.is_empty()
        || (!data.is_empty()
            && content_type.as_deref().is_none_or(|ct| ct.contains("x-www-form-urlencoded"))
            && data.iter().all(|d| looks_like_form(d)))
    {
        let mut fields: Vec<KeyValue> = data.iter().flat_map(|d| url_sync::parse_query(d)).map(decode_kv).collect();
        fields.extend(urlencode.iter().map(|p| match p.split_once('=') {
            Some((k, v)) => KeyValue::new(k, v),
            None => KeyValue::new(p.as_str(), ""),
        }));
        // Тип тела задаётся вкладкой Body, дублирующий заголовок не нужен.
        headers.retain(|h| !h.key.eq_ignore_ascii_case("content-type"));
        body = Body::UrlEncoded(fields);
    } else if !data.is_empty() {
        let text = data.join("&");
        let kind = match content_type.as_deref() {
            Some(ct) if ct.contains("json") => RawKind::Json,
            Some(ct) if ct.contains("xml") => RawKind::Xml,
            Some(ct) if ct.contains("html") => RawKind::Html,
            Some(ct) if ct.contains("javascript") => RawKind::JavaScript,
            Some(_) => RawKind::Text,
            None if serde_json::from_str::<serde_json::Value>(&text).is_ok() => RawKind::Json,
            None => RawKind::Text,
        };
        if content_type.as_deref().is_some_and(|ct| ct == kind.content_type()) {
            headers.retain(|h| !h.key.eq_ignore_ascii_case("content-type"));
        }
        body = Body::Raw { text, kind };
    }

    let has_body = !matches!(body, Body::None);
    let method = match method {
        Some(m) => Method::STANDARD.iter().find(|s| s.as_str() == m).cloned().unwrap_or(Method::Custom(m)),
        None if has_body => Method::Post,
        None => Method::Get,
    };
    let name = request_name(&method, &url);
    Ok(Request {
        name,
        method,
        query: url_sync::params_from_url(&url, &[]),
        url,
        headers,
        body,
        auth,
        settings,
        ..Request::default()
    })
}

/// «GET /users» из адреса — понятное имя вкладки.
pub(crate) fn request_name(method: &Method, url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = without_scheme.find('/').map_or("/", |i| &without_scheme[i..]);
    let path = url_sync::split_url(path).0;
    format!("{method} {path}")
}

fn looks_like_form(data: &str) -> bool {
    !data.trim_start().starts_with(['{', '[', '<']) && data.split('&').all(|p| p.contains('=') && !p.contains(' '))
}

fn decode_kv(kv: KeyValue) -> KeyValue {
    let decode = |s: &str| {
        url::form_urlencoded::parse(format!("x={s}").as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_default()
    };
    KeyValue::new(decode(&kv.key), decode(&kv.value))
}

fn encode_pair(pair: &str) -> String {
    let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    if k.is_empty() {
        enc(v)
    } else {
        format!("{k}={}", enc(v))
    }
}

/// Делит строку на аргументы по правилам shell: кавычки ' и ", `$'…'`, экранирование
/// и перенос строки через `\` (bash) или `^` (cmd).
fn split_args(command: &str) -> Result<Vec<String>, ImportError> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => current.push(ch),
                        None => return Err(ImportError::new("не закрыта одинарная кавычка")),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_arg = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => current.push('\n'),
                            Some('t') => current.push('\t'),
                            Some('r') => current.push('\r'),
                            Some(ch) => current.push(ch),
                            None => return Err(ImportError::new("не закрыта кавычка $'…'")),
                        },
                        Some(ch) => current.push(ch),
                        None => return Err(ImportError::new("не закрыта кавычка $'…'")),
                    }
                }
            }
            '"' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\' | '$' | '`')) => current.push(ch),
                            Some('\n') => {}
                            Some(ch) => {
                                current.push('\\');
                                current.push(ch);
                            }
                            None => return Err(ImportError::new("не закрыта двойная кавычка")),
                        },
                        Some(ch) => current.push(ch),
                        None => return Err(ImportError::new("не закрыта двойная кавычка")),
                    }
                }
            }
            '\\' | '^' if matches!(chars.peek(), Some('\n' | '\r')) => {
                while matches!(chars.peek(), Some('\n' | '\r')) {
                    chars.next();
                }
            }
            '\\' => {
                if let Some(ch) = chars.next() {
                    current.push(ch);
                    in_arg = true;
                }
            }
            c if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            c => {
                current.push(c);
                in_arg = true;
            }
        }
    }
    if in_arg {
        args.push(current);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use queryman_core::model::RequestSettings;
    use queryman_core::prepare::Part;

    fn req(method: &str) -> ResolvedRequest {
        ResolvedRequest {
            method: method.into(),
            url: "http://localhost/a?b=1".into(),
            headers: Vec::new(),
            body: None,
            settings: RequestSettings { follow_redirects: false, ..Default::default() },
        }
    }

    #[test]
    fn plain_get() {
        assert_eq!(to_curl(&req("GET")), "curl \\\n  'http://localhost/a?b=1'");
    }

    #[test]
    fn post_with_headers_body_and_quotes() {
        let mut r = req("POST");
        r.headers.push(("Content-Type".into(), "application/json".into()));
        r.body = Some(ResolvedBody::Bytes(br#"{"name":"it's"}"#.to_vec()));
        let cmd = to_curl(&r);
        assert!(cmd.contains("-X POST"));
        assert!(cmd.contains("-H 'Content-Type: application/json'"));
        assert!(cmd.contains(r#"--data-raw '{"name":"it'\''s"}'"#));
    }

    #[test]
    fn files_become_form_and_binary_flags() {
        let mut r = req("POST");
        r.body = Some(ResolvedBody::Multipart(vec![
            Part { name: "a".into(), value: PartValue::Text("1".into()) },
            Part { name: "f".into(), value: PartValue::File("/tmp/x.png".into()) },
        ]));
        let cmd = to_curl(&r);
        assert!(cmd.contains("-F 'a=1'") && cmd.contains("-F 'f=@/tmp/x.png'"), "{cmd}");
        r.body = Some(ResolvedBody::File("/tmp/d.bin".into()));
        assert!(to_curl(&r).contains("--data-binary '@/tmp/d.bin'"));
    }

    #[test]
    fn settings_become_flags() {
        let mut r = req("GET");
        r.settings =
            RequestSettings { timeout_ms: Some(1500), follow_redirects: true, max_redirects: 5, verify_tls: false };
        let cmd = to_curl(&r);
        assert!(cmd.contains("-L --max-redirs 5"));
        assert!(cmd.contains("--insecure"));
        assert!(cmd.contains("--max-time 1.5"));
    }

    #[test]
    fn import_json_post_from_bash() {
        let cmd = r#"curl -X POST 'https://api.example.com/users?page=2' \
  -H 'Content-Type: application/json' \
  -H "Authorization: Bearer abc" \
  --data-raw '{"name":"it'\''s"}' --compressed -s"#;
        let r = from_curl(cmd).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url, "https://api.example.com/users?page=2");
        assert_eq!(r.name, "POST /users");
        assert_eq!(r.query, vec![KeyValue::new("page", "2")]);
        assert_eq!(r.headers, vec![KeyValue::new("Authorization", "Bearer abc")]);
        assert_eq!(r.body, Body::Raw { text: r#"{"name":"it's"}"#.into(), kind: RawKind::Json });
    }

    #[test]
    fn import_form_data_user_and_flags() {
        let cmd = "curl -L -k -u admin:secret -F 'file=@/tmp/a.png;type=image/png' -F name=x \
                   --max-time 2.5 http://localhost:3000/upload";
        let r = from_curl(cmd).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.auth, Auth::Basic { username: "admin".into(), password: "secret".into() });
        assert_eq!(r.body, Body::FormData(vec![FormField::file("file", "/tmp/a.png"), FormField::text("name", "x")]));
        assert!(r.settings.follow_redirects && !r.settings.verify_tls);
        assert_eq!(r.settings.timeout_ms, Some(2500));
    }

    #[test]
    fn import_urlencoded_binary_get_and_cmd() {
        let r = from_curl("curl https://x/login -d 'user=a%40b.c' -d pass=1").unwrap();
        assert_eq!(r.body, Body::UrlEncoded(vec![KeyValue::new("user", "a@b.c"), KeyValue::new("pass", "1")]));

        let r = from_curl("curl -XPUT --data-binary @payload.bin https://x/blob").unwrap();
        assert_eq!(r.method, Method::Put);
        assert_eq!(r.body, Body::Binary { path: "payload.bin".into() });

        let r = from_curl("curl -G https://x/search --data-urlencode 'q=a b'").unwrap();
        assert_eq!((r.method, r.url.as_str(), r.body), (Method::Get, "https://x/search?q=a+b", Body::None));

        let r = from_curl("curl \"https://x/a\" ^\r\n  -H \"Accept: */*\"").unwrap();
        assert_eq!(r.headers, vec![KeyValue::new("Accept", "*/*")]);

        assert!(from_curl("wget http://x").is_err());
        assert!(from_curl("curl -H 'a: b'").is_err());
        assert!(from_curl("curl 'http://x").is_err());
    }
}
