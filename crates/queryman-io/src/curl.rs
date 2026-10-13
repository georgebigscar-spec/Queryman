use queryman_core::prepare::ResolvedRequest;

/// Команда cURL, которая выполняет тот же запрос.
/// Значения авторизации уже вычислены — команда может содержать секреты.
pub fn to_curl(req: &ResolvedRequest) -> String {
    let mut parts = vec!["curl".to_owned()];
    if req.method != "GET" || req.body.is_some() {
        parts.push(format!("-X {}", req.method));
    }
    parts.push(quote(&req.url));
    for (name, value) in &req.headers {
        parts.push(format!("-H {}", quote(&format!("{name}: {value}"))));
    }
    if let Some(body) = &req.body {
        parts.push(format!("--data-raw {}", quote(&String::from_utf8_lossy(body))));
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

#[cfg(test)]
mod tests {
    use super::*;
    use queryman_core::model::RequestSettings;

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
        r.body = Some(br#"{"name":"it's"}"#.to_vec());
        let cmd = to_curl(&r);
        assert!(cmd.contains("-X POST"));
        assert!(cmd.contains("-H 'Content-Type: application/json'"));
        assert!(cmd.contains(r#"--data-raw '{"name":"it'\''s"}'"#));
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
}
