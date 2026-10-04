//! The HTTP client: construction, credentials and the request pipeline.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};
use reqwest_cookie_store::CookieStoreMutex;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;
use tracing::Instrument as _;
use url::Url;

use crate::context::Context;
use crate::error::{Error, Result, truncate};
use crate::ratelimit::{
    DEFAULT_MIN_INTERVAL, Pacer, RetryConfig, is_replayable, retry_after as read_retry_after,
};
use crate::session::{Session, Tokens};
use crate::types::InvalidValue;

/// The website, which serves the basket, orders and the page JSON.
pub const DEFAULT_BASE_URL: &str = "https://www.nemlig.com/";

/// The search gateway, a separate service with its own host and its own rules.
pub const DEFAULT_SEARCH_BASE_URL: &str = "https://webapi.prod.knl.nemlig.it/searchgateway/api/";

/// What this crate calls itself.
///
/// A browser string rather than a crate name: the site's edge rejects clients it does not
/// recognise, and the captured traffic this crate was written against sends this one.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

/// The web app version announced in the `Version` header.
///
/// Tracks nemlig.com's own front end rather than this crate, and may need raising if the
/// site starts refusing it.
pub const WEB_APP_VERSION: &str = "11.201.0";

/// How long a request may take before it is abandoned.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the page settings are reused before being fetched again.
///
/// They carry the delivery timeslot, which decides which prices search returns, so this
/// is short enough that a slot changing under the client is noticed within the minute.
pub const DEFAULT_CONTEXT_TTL: Duration = Duration::from_secs(300);

const DEVICE_SIZE: HeaderName = HeaderName::from_static("device-size");
const PLATFORM: HeaderName = HeaderName::from_static("platform");
const VERSION: HeaderName = HeaderName::from_static("version");
const CORRELATION_ID: HeaderName = HeaderName::from_static("x-correlation-id");
const XSRF_TOKEN: HeaderName = HeaderName::from_static("x-xsrf-token");

const ACCEPT_JSON: HeaderValue = HeaderValue::from_static("application/json, text/plain, */*");
const JSON: HeaderValue = HeaderValue::from_static("application/json");

/// A value that must not reach a log, an error message or a `Debug` dump.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Read the value. The one place it escapes, and so the one place to grep for.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// A nemlig.com account.
#[derive(Clone)]
pub struct Credentials {
    pub(crate) username: String,
    pub(crate) password: Secret,
}

impl Credentials {
    /// The email address and password used on nemlig.com itself.
    pub fn new(username: impl Into<String>, password: impl Into<Secret>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// The account's email address.
    pub fn username(&self) -> &str {
        &self.username
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &self.password)
            .finish()
    }
}

/// Which of nemlig.com's two services a request goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    /// `www.nemlig.com`, which wants the full header set and the anti-forgery token.
    Site,
    /// The search gateway, which wants the bearer token and nothing else.
    Search,
}

/// A client for the nemlig.com API.
///
/// Cloning is cheap and shares everything that matters: the connection pool, the signed-in
/// session, the pacing queue and the cached page settings. Two clones cannot end up signed
/// in as different accounts or race each other past the pacer.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    base: Url,
    search_base: Url,
    user_agent: HeaderValue,
    credentials: Credentials,
    session: Session,
    context: RwLock<Option<(Context, Instant)>>,
    context_ttl: Duration,
    pacer: Pacer,
    retry: RetryConfig,
    requests: AtomicU64,
    allow_place_order: bool,
    /// Held while signing in, so a burst of requests produces one login rather than ten.
    auth: tokio::sync::Mutex<()>,
}

impl Client {
    /// Start building a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Build a client for an account with everything else left at its default.
    pub fn new(credentials: Credentials) -> Result<Self> {
        Self::builder().credentials(credentials).build()
    }

    /// The account this client signs in as.
    pub fn credentials(&self) -> &Credentials {
        &self.inner.credentials
    }

    /// How many requests this client and its clones have sent, retries included.
    pub fn requests_made(&self) -> u64 {
        self.inner.requests.load(Ordering::Relaxed)
    }

    /// Whether order placement was enabled on this client.
    pub fn place_order_allowed(&self) -> bool {
        self.inner.allow_place_order
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    pub(crate) fn session(&self) -> &Session {
        &self.inner.session
    }

    /// Serialise sign-ins across every clone of this client.
    pub(crate) async fn auth_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.auth.lock().await
    }

    /// A URL on the website, from a path that may carry its own query string.
    pub(crate) fn site_url(&self, path: &str) -> Result<Url> {
        Ok(self.inner.base.join(path)?)
    }

    pub(crate) fn count_request(&self) {
        self.inner.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) async fn pace(&self) {
        self.inner.pacer.acquire().await;
    }

    /// Build a URL on one of the two backends from already-decoded path segments.
    pub(crate) fn url(&self, backend: Backend, segments: &[&str]) -> Result<Url> {
        let mut url = match backend {
            Backend::Site => self.inner.base.clone(),
            Backend::Search => self.inner.search_base.clone(),
        };
        {
            // `cannot_be_a_base` is the only way this fails, and the builder rejected
            // such a base already, so the error is folded into the loop below instead.
            let Ok(mut path) = url.path_segments_mut() else {
                return Err(Error::Url(url::ParseError::RelativeUrlWithoutBase));
            };
            // Both bases end in a slash, which is an empty final segment; pushing onto it
            // would double the slash. Only when there is something to push, so the bare
            // base keeps the root path it needs for the page JSON.
            if !segments.is_empty() {
                path.pop_if_empty();
            }
            for segment in segments {
                check_segment(segment)?;
                path.push(segment);
            }
        }
        Ok(url)
    }

    /// A GET on one of the two backends.
    pub(crate) fn get(&self, backend: Backend, segments: &[&str]) -> Result<Req> {
        Ok(Req::new(backend, Method::GET, self.url(backend, segments)?))
    }

    /// A POST on the website.
    pub(crate) fn post(&self, segments: &[&str]) -> Result<Req> {
        Ok(Req::new(
            Backend::Site,
            Method::POST,
            self.url(Backend::Site, segments)?,
        ))
    }

    /// Send a request, signing in first if the session is absent or expiring.
    pub(crate) async fn send(&self, req: Req) -> Result<Res> {
        let path = req.url.path().to_owned();
        let span = tracing::debug_span!(
            "nemlig.request",
            http.method = %req.method,
            url.path = %path,
        );
        self.send_inner(req).instrument(span).await
    }

    /// Send a request and decode its body, treating a 404 as "there is none".
    pub(crate) async fn send_json_opt<T: DeserializeOwned>(
        &self,
        req: Req,
        expected: &'static str,
    ) -> Result<Option<T>> {
        match self.send(req).await {
            // An endpoint that answers 204 or an empty 200 is saying the same thing a 404
            // does, and nemlig.com uses all three for "nothing booked".
            Ok(res) if res.text().trim().is_empty() => Ok(None),
            Ok(res) => res.json(expected).map(Some),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Send a request and decode its body.
    pub(crate) async fn send_json<T: DeserializeOwned>(
        &self,
        req: Req,
        expected: &'static str,
    ) -> Result<T> {
        self.send(req).await?.json(expected)
    }

    async fn send_inner(&self, req: Req) -> Result<Res> {
        let mut reauthenticated = false;
        let mut attempt = 0;

        loop {
            let tokens = self.tokens().await?;
            self.pace().await;

            let built = self.build(&req, &tokens)?;
            self.count_request();
            let response = self.inner.http.execute(built).await;

            let response = match response {
                Ok(response) => response,
                Err(source) => {
                    if is_replayable(&req.method) && attempt < self.inner.retry.max_retries {
                        let delay = self.inner.retry.backoff(attempt);
                        tracing::warn!(?delay, "request failed before a response; retrying");
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::transport(
                        format!("{} {}", req.method, req.url.path()),
                        source,
                    ));
                }
            };

            let status = response.status();
            let headers = response.headers().clone();
            let body = response.text().await.map_err(|source| {
                Error::transport(format!("reading {} {}", req.method, req.url.path()), source)
            })?;
            tracing::debug!(status = status.as_u16(), bytes = body.len(), "response");

            if status.is_success() {
                return Ok(Res { body });
            }

            // A 401 means the request was refused before it was processed, so resending it
            // is safe whatever the method. Signing in again covers the case the refresh
            // cannot: the year-long cookie expired, or the account was signed out elsewhere.
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                && !reauthenticated
            {
                tracing::info!(
                    status = status.as_u16(),
                    "session refused; signing in again"
                );
                self.inner.session.invalidate().await;
                reauthenticated = true;
                continue;
            }

            if status == StatusCode::TOO_MANY_REQUESTS {
                let wait = read_retry_after(&headers).unwrap_or(self.inner.retry.backoff(attempt));
                if attempt < self.inner.retry.max_retries && wait <= self.inner.retry.max_delay {
                    tracing::warn!(?wait, "rate limited; waiting");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                    continue;
                }
                return Err(Error::RateLimited { retry_after: wait });
            }

            if status.is_server_error()
                && is_replayable(&req.method)
                && attempt < self.inner.retry.max_retries
            {
                let delay = self.inner.retry.backoff(attempt);
                tracing::warn!(status = status.as_u16(), ?delay, "server error; retrying");
                tokio::time::sleep(delay).await;
                attempt += 1;
                continue;
            }

            return Err(Error::Api {
                status: status.as_u16(),
                method: method_name(&req.method),
                path: req.url.path().to_owned(),
                body: truncate(&body),
            });
        }
    }

    /// Turn a [`Req`] into a reqwest request, with the headers its backend expects.
    fn build(&self, req: &Req, tokens: &Tokens) -> Result<reqwest::Request> {
        let mut url = req.url.clone();
        if !req.query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in &req.query {
                pairs.append_pair(key, value);
            }
        }

        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, ACCEPT_JSON);
        headers.insert(header::USER_AGENT, self.user_agent());
        headers.insert(CORRELATION_ID, correlation_id());
        headers.insert(header::REFERER, req.referer(&self.inner.base)?);

        let mut bearer = HeaderValue::from_str(&format!("Bearer {}", tokens.bearer.expose()))
            .map_err(|_| header_rejected("bearer token"))?;
        bearer.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, bearer);

        if req.backend == Backend::Site {
            headers.insert(header::CONTENT_TYPE, JSON);
            headers.insert(DEVICE_SIZE, HeaderValue::from_static("desktop"));
            headers.insert(PLATFORM, HeaderValue::from_static("web"));
            headers.insert(VERSION, HeaderValue::from_static(WEB_APP_VERSION));
            headers.insert(
                XSRF_TOKEN,
                HeaderValue::from_str(&tokens.xsrf).map_err(|_| header_rejected("xsrf token"))?,
            );
        }

        let mut builder = self
            .inner
            .http
            .request(req.method.clone(), url)
            .headers(headers);
        if let Some(body) = &req.body {
            builder = builder.body(serde_json::to_vec(body).map_err(Error::Encode)?);
        }
        builder.build().map_err(|source| {
            Error::transport(
                format!("building {} {}", req.method, req.url.path()),
                source,
            )
        })
    }

    /// The agent string, rendered once at build time.
    ///
    /// reqwest sets it too, but each request here replaces the header map wholesale, so
    /// the value has to be put back by hand.
    fn user_agent(&self) -> HeaderValue {
        self.inner.user_agent.clone()
    }

    /// The cached page settings, fetched if absent or past their time to live.
    pub(crate) async fn context(&self) -> Result<Context> {
        if let Some((context, fetched)) = self.inner.context.read().await.as_ref()
            && fetched.elapsed() < self.inner.context_ttl
        {
            return Ok(context.clone());
        }

        let fresh = self.fetch_context().await?;
        *self.inner.context.write().await = Some((fresh.clone(), Instant::now()));
        Ok(fresh)
    }

    /// Drop the cached page settings, so the next read fetches them again.
    pub(crate) async fn forget_context(&self) {
        *self.inner.context.write().await = None;
    }
}

/// A request under construction.
#[derive(Debug)]
pub(crate) struct Req {
    backend: Backend,
    method: Method,
    url: Url,
    query: Vec<(String, String)>,
    body: Option<serde_json::Value>,
    referer: Option<&'static str>,
}

impl Req {
    fn new(backend: Backend, method: Method, url: Url) -> Self {
        Self {
            backend,
            method,
            url,
            query: Vec::new(),
            body: None,
            referer: None,
        }
    }

    /// Add a query parameter. Repeating a key repeats it on the wire.
    pub(crate) fn query(mut self, key: &str, value: impl ToString) -> Self {
        self.query.push((key.to_owned(), value.to_string()));
        self
    }

    /// Add a query parameter only when there is one to add.
    pub(crate) fn maybe_query(self, key: &str, value: Option<impl ToString>) -> Self {
        match value {
            Some(value) => self.query(key, value),
            None => self,
        }
    }

    /// Send a JSON body.
    pub(crate) fn json<T: Serialize>(mut self, body: &T) -> Result<Self> {
        self.body = Some(serde_json::to_value(body).map_err(Error::Encode)?);
        Ok(self)
    }

    /// Override the `Referer`, which some endpoints check.
    pub(crate) fn referrer(mut self, path: &'static str) -> Self {
        self.referer = Some(path);
        self
    }

    fn referer(&self, base: &Url) -> Result<HeaderValue> {
        let url = match self.referer {
            Some(path) => base.join(path)?,
            None => base.clone(),
        };
        HeaderValue::from_str(url.as_str()).map_err(|_| header_rejected("referer"))
    }
}

/// A successful response, read into memory.
#[derive(Debug)]
pub(crate) struct Res {
    body: String,
}

impl Res {
    /// Decode the body, naming what was expected so a failure says what broke.
    pub(crate) fn json<T: DeserializeOwned>(&self, expected: &'static str) -> Result<T> {
        serde_json::from_str(&self.body).map_err(|source| Error::Decode {
            expected,
            body: truncate(&self.body),
            source,
        })
    }

    /// The raw body, for the endpoints that answer with something other than JSON.
    pub(crate) fn text(&self) -> &str {
        &self.body
    }
}

/// Builds a [`Client`].
#[derive(Debug)]
pub struct ClientBuilder {
    credentials: Option<Credentials>,
    base: String,
    search_base: String,
    user_agent: String,
    timeout: Duration,
    min_interval: Duration,
    retry: RetryConfig,
    session_file: Option<PathBuf>,
    context_ttl: Duration,
    allow_place_order: bool,
    http: Option<reqwest::ClientBuilder>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            credentials: None,
            base: DEFAULT_BASE_URL.to_owned(),
            search_base: DEFAULT_SEARCH_BASE_URL.to_owned(),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            min_interval: DEFAULT_MIN_INTERVAL,
            retry: RetryConfig::default(),
            session_file: None,
            context_ttl: DEFAULT_CONTEXT_TTL,
            allow_place_order: false,
            http: None,
        }
    }
}

impl ClientBuilder {
    /// The account to sign in as. Required.
    pub fn credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Point the website calls somewhere else, such as a mock server.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base = url.into();
        self
    }

    /// Point the search calls somewhere else, such as a mock server.
    pub fn search_base_url(mut self, url: impl Into<String>) -> Self {
        self.search_base = url.into();
        self
    }

    /// Announce a different `User-Agent`.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// How long one request may take.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Shortest gap between requests. [`Duration::ZERO`] turns pacing off.
    pub fn min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }

    /// How many times a read is retried after a failure the client did not cause.
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.retry.max_retries = retries;
        self
    }

    /// The longest pause the client will take before giving up instead.
    pub fn max_retry_delay(mut self, delay: Duration) -> Self {
        self.retry.max_delay = delay;
        self
    }

    /// Keep the session in a file, so the next process skips the login handshake.
    ///
    /// The file holds the account's year-long authentication cookie. It is written so
    /// only its owner can read it, but it is still a credential: treat the path the way
    /// you would an SSH key.
    pub fn session_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.session_file = Some(path.into());
        self
    }

    /// How long the page settings are reused before being fetched again.
    pub fn context_ttl(mut self, ttl: Duration) -> Self {
        self.context_ttl = ttl;
        self
    }

    /// Permit order placement, which spends real money.
    ///
    /// Without this, `checkout().place_order()` refuses before sending anything. It also
    /// needs the `place-order` crate feature, which is not in the default set, so
    /// enabling it takes two deliberate steps.
    pub fn allow_place_order(mut self, allow: bool) -> Self {
        self.allow_place_order = allow;
        self
    }

    /// Supply the reqwest builder the HTTP client is made from.
    ///
    /// The cookie jar and the timeout are applied on top, since the session depends on
    /// both. Everything else -- proxies, a different TLS backend, connection limits --
    /// is yours to set.
    pub fn http_builder(mut self, builder: reqwest::ClientBuilder) -> Self {
        self.http = Some(builder);
        self
    }

    /// Build the client.
    pub fn build(self) -> Result<Client> {
        let credentials = self.credentials.ok_or(InvalidValue {
            field: "credentials",
            reason: "required",
            value: String::new(),
        })?;

        let base = parse_base("base_url", &self.base)?;
        let search_base = parse_base("search_base_url", &self.search_base)?;

        let user_agent = HeaderValue::from_str(&self.user_agent).map_err(|_| InvalidValue {
            field: "user_agent",
            reason: "is not a valid header value",
            value: self.user_agent.clone(),
        })?;

        let cookies = Arc::new(CookieStoreMutex::new(cookie_store::CookieStore::default()));
        let http = self
            .http
            .unwrap_or_default()
            .cookie_provider(Arc::clone(&cookies))
            .timeout(self.timeout)
            .user_agent(&self.user_agent)
            .build()
            .map_err(|source| Error::transport("building the HTTP client", source))?;

        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base,
                search_base,
                user_agent,
                credentials,
                session: Session::new(cookies, self.session_file),
                context: RwLock::new(None),
                context_ttl: self.context_ttl,
                pacer: Pacer::new(self.min_interval),
                retry: self.retry,
                requests: AtomicU64::new(0),
                allow_place_order: self.allow_place_order,
                auth: tokio::sync::Mutex::new(()),
            }),
        })
    }
}

/// Parse a base URL, insisting on the trailing slash that path joining needs.
fn parse_base(field: &'static str, raw: &str) -> Result<Url> {
    let with_slash = if raw.ends_with('/') {
        raw.to_owned()
    } else {
        format!("{raw}/")
    };
    let url = Url::parse(&with_slash)?;
    if url.cannot_be_a_base() {
        return Err(InvalidValue {
            field,
            reason: "cannot have path segments appended",
            value: raw.to_owned(),
        }
        .into());
    }
    Ok(url)
}

/// Reject segments the `url` crate would silently fold away.
fn check_segment(segment: &str) -> Result<()> {
    let bad = |reason| {
        Err(Error::Invalid(InvalidValue {
            field: "path segment",
            reason,
            value: segment.to_owned(),
        }))
    };
    match segment {
        "" => bad("empty"),
        "." | ".." => bad("traverses the path"),
        s if s.contains('/') => bad("contains a separator"),
        _ => Ok(()),
    }
}

/// A fresh request identifier, which nemlig.com uses for tracing on its own side.
fn correlation_id() -> HeaderValue {
    let id = uuid::Uuid::new_v4().to_string();
    HeaderValue::from_str(&id).unwrap_or(HeaderValue::from_static(
        "00000000-0000-0000-0000-000000000000",
    ))
}

fn header_rejected(what: &'static str) -> Error {
    Error::Invalid(InvalidValue {
        field: what,
        reason: "is not a valid header value",
        value: String::new(),
    })
}

fn method_name(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::DELETE => "DELETE",
        _ => "HTTP",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn client() -> Client {
        Client::new(Credentials::new(
            "someone@example.com",
            "hunter2-not-a-real-password",
        ))
        .expect("a default client builds")
    }

    #[test]
    fn a_client_without_credentials_does_not_build() {
        let err = Client::builder()
            .build()
            .expect_err("credentials are required");
        assert!(matches!(err, Error::Invalid(_)));
    }

    #[test]
    fn a_secret_hides_itself_from_every_rendering() {
        let secret = Secret::new("a-real-password");
        assert_eq!(format!("{secret}"), "<redacted>");
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert!(!format!("{:?}", Credentials::new("a@b.c", secret.clone())).contains("a-real"));
        assert_eq!(secret.expose(), "a-real-password");
    }

    #[test]
    fn path_segments_are_joined_onto_the_right_backend() {
        let client = client();
        assert_eq!(
            client
                .url(Backend::Site, &["webapi", "basket", "GetBasket"])
                .expect("builds")
                .as_str(),
            "https://www.nemlig.com/webapi/basket/GetBasket"
        );
        assert_eq!(
            client
                .url(Backend::Search, &["search"])
                .expect("builds")
                .as_str(),
            "https://webapi.prod.knl.nemlig.it/searchgateway/api/search"
        );
    }

    #[test]
    fn path_segments_are_percent_encoded() {
        let url = client()
            .url(Backend::Site, &["øl og vand-123"])
            .expect("builds");
        assert_eq!(url.path(), "/%C3%B8l%20og%20vand-123");
    }

    #[test]
    fn a_segment_that_would_escape_the_path_is_refused() {
        for bad in ["", ".", "..", "a/b"] {
            assert!(
                client().url(Backend::Site, &[bad]).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn a_base_url_without_a_trailing_slash_still_joins() {
        let client = Client::builder()
            .credentials(Credentials::new("a@b.c", "x"))
            .base_url("http://127.0.0.1:1234")
            .build()
            .expect("builds");
        assert_eq!(
            client
                .url(Backend::Site, &["webapi", "Token"])
                .expect("builds")
                .as_str(),
            "http://127.0.0.1:1234/webapi/Token"
        );
    }

    #[test]
    fn the_referer_defaults_to_the_site_root() {
        let base = Url::parse(DEFAULT_BASE_URL).expect("valid");
        let req = Req::new(Backend::Site, Method::GET, base.clone());
        assert_eq!(
            req.referer(&base).expect("builds"),
            "https://www.nemlig.com/"
        );
        assert_eq!(
            req.referrer("basket").referer(&base).expect("builds"),
            "https://www.nemlig.com/basket"
        );
    }

    #[test]
    fn place_order_is_off_unless_asked_for() {
        assert!(!client().place_order_allowed());
    }
}
