//! Выполнение запросов. Вызывается только из рабочего потока (раздел 3).

use queryman_core::model::{RequestSettings, ResponseData};
use queryman_core::prepare::ResolvedRequest;
use queryman_core::protocol::HttpError;
use reqwest::redirect::Policy;
use std::collections::HashMap;
use std::error::Error as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const USER_AGENT: &str = concat!("Queryman/", env!("CARGO_PKG_VERSION"));

/// Параметры, которые в reqwest задаются на уровне клиента, а не запроса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ClientKey {
    verify_tls: bool,
    follow_redirects: bool,
    max_redirects: u8,
}

/// Набор клиентов с общим cookie jar (FR-70). Клиент дешёво клонируется.
pub struct ClientPool {
    jar: Arc<reqwest::cookie::Jar>,
    clients: HashMap<ClientKey, reqwest::Client>,
}

impl Default for ClientPool {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientPool {
    pub fn new() -> Self {
        Self { jar: Arc::new(reqwest::cookie::Jar::default()), clients: HashMap::new() }
    }

    pub fn get(&mut self, settings: &RequestSettings) -> Result<reqwest::Client, HttpError> {
        let key = ClientKey {
            verify_tls: settings.verify_tls,
            follow_redirects: settings.follow_redirects,
            max_redirects: settings.max_redirects,
        };
        if let Some(client) = self.clients.get(&key) {
            return Ok(client.clone());
        }
        let redirect =
            if key.follow_redirects { Policy::limited(usize::from(key.max_redirects)) } else { Policy::none() };
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(self.jar.clone())
            .danger_accept_invalid_certs(!key.verify_tls)
            .redirect(redirect)
            .build()
            .map_err(|e| HttpError::Other(error_chain(&e)))?;
        self.clients.insert(key, client.clone());
        Ok(client)
    }
}

/// Отправляет запрос и читает ответ целиком.
pub async fn execute(client: &reqwest::Client, req: ResolvedRequest) -> Result<ResponseData, HttpError> {
    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|_| HttpError::InvalidRequest(format!("метод «{}»", req.method)))?;
    if req.url.trim().is_empty() {
        return Err(HttpError::InvalidUrl("адрес не указан".to_owned()));
    }
    let url = reqwest::Url::parse(&req.url).map_err(|e| HttpError::InvalidUrl(format!("{} ({e})", req.url)))?;
    let timeout_ms = req.settings.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);

    let mut builder = client.request(method, url).timeout(Duration::from_millis(timeout_ms));
    for (name, value) in &req.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    if let Some(body) = req.body {
        builder = builder.body(body);
    }

    let started = Instant::now();
    let response = builder.send().await.map_err(|e| map_error(&e, timeout_ms))?;
    let ttfb = started.elapsed();

    let status = response.status();
    let http_version = format!("{:?}", response.version());
    let final_url = response.url().to_string();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let body = response.bytes().await.map_err(|e| map_error(&e, timeout_ms))?.to_vec();
    let elapsed = started.elapsed();

    Ok(ResponseData {
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or_default().to_owned(),
        http_version,
        headers,
        size_bytes: body.len() as u64,
        body,
        elapsed_ms: elapsed.as_millis() as u64,
        ttfb_ms: ttfb.as_millis() as u64,
        final_url,
    })
}

fn map_error(e: &reqwest::Error, timeout_ms: u64) -> HttpError {
    if e.is_timeout() {
        HttpError::Timeout(timeout_ms)
    } else if e.is_redirect() {
        HttpError::TooManyRedirects
    } else if e.is_builder() {
        HttpError::InvalidRequest(error_chain(e))
    } else if e.is_connect() {
        HttpError::Connect(error_chain(e))
    } else {
        HttpError::Other(error_chain(e))
    }
}

/// Текст ошибки вместе с причинами: «error sending request: … : Connection refused».
fn error_chain(e: &reqwest::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request(method: &str, url: String) -> ResolvedRequest {
        ResolvedRequest {
            method: method.to_owned(),
            url,
            headers: Vec::new(),
            body: None,
            settings: RequestSettings::default(),
        }
    }

    #[tokio::test]
    async fn get_returns_status_headers_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users"))
            .respond_with(
                // set_body_string подставил бы text/plain, поэтому тип задаётся вместе с телом.
                ResponseTemplate::new(200).set_body_raw("[1,2]", "application/json"),
            )
            .mount(&server)
            .await;

        let client = ClientPool::new().get(&RequestSettings::default()).unwrap();
        let resp = execute(&client, request("GET", format!("{}/users", server.uri()))).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.reason, "OK");
        assert!(resp.is_json());
        assert_eq!(resp.body, b"[1,2]");
        assert_eq!(resp.size_bytes, 5);
    }

    #[tokio::test]
    async fn post_sends_headers_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/echo"))
            .and(header("content-type", "application/json"))
            .and(body_string(r#"{"a":1}"#))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let mut req = request("POST", format!("{}/echo", server.uri()));
        req.headers.push(("Content-Type".into(), "application/json".into()));
        req.body = Some(br#"{"a":1}"#.to_vec());
        let client = ClientPool::new().get(&req.settings).unwrap();
        assert_eq!(execute(&client, req).await.unwrap().status, 201);
    }

    #[tokio::test]
    async fn timeout_is_reported() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
            .mount(&server)
            .await;

        let mut req = request("GET", server.uri());
        req.settings.timeout_ms = Some(100);
        let client = ClientPool::new().get(&req.settings).unwrap();
        assert_eq!(execute(&client, req).await.unwrap_err(), HttpError::Timeout(100));
    }

    #[tokio::test]
    async fn connection_refused_and_bad_url() {
        let client = ClientPool::new().get(&RequestSettings::default()).unwrap();
        let err = execute(&client, request("GET", "http://127.0.0.1:1/".into())).await.unwrap_err();
        assert!(matches!(err, HttpError::Connect(_)), "{err:?}");
        let err = execute(&client, request("GET", "not a url".into())).await.unwrap_err();
        assert!(matches!(err, HttpError::InvalidUrl(_)), "{err:?}");
    }

    #[tokio::test]
    async fn redirects_follow_policy() {
        let server = MockServer::start().await;
        Mock::given(path("/old"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/new"))
            .mount(&server)
            .await;
        Mock::given(path("/new")).respond_with(ResponseTemplate::new(200)).mount(&server).await;

        let mut pool = ClientPool::new();
        let mut req = request("GET", format!("{}/old", server.uri()));
        let follow = pool.get(&req.settings).unwrap();
        let resp = execute(&follow, req.clone()).await.unwrap();
        assert_eq!(resp.status, 200);
        assert!(resp.final_url.ends_with("/new"));

        req.settings.follow_redirects = false;
        let no_follow = pool.get(&req.settings).unwrap();
        assert_eq!(execute(&no_follow, req).await.unwrap().status, 302);
    }
}
