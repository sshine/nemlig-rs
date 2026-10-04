//! The signed-in session: cookies, the bearer token and the anti-forgery token.
//!
//! nemlig.com hands out three things at login that are needed together. The `.ASPXAUTH`
//! cookie identifies the account for a year, the bearer token authorises each request for
//! five minutes, and the anti-forgery token has to accompany every write. Only the bearer
//! expires on a timescale that matters mid-run, so the client refreshes that one on its
//! own and falls back to a full login when the cookie has gone too.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::client::Secret;
use crate::error::{Error, Result};

/// How close to expiry a bearer token is refreshed rather than used.
///
/// Five minutes is the whole lifetime, so this is generous on purpose: a request that
/// sets out with four seconds left can still arrive after the token has died.
pub(crate) const REFRESH_MARGIN: TimeDelta = TimeDelta::seconds(30);

/// What a signed-in client holds besides its cookies.
#[derive(Debug, Clone)]
pub(crate) struct Tokens {
    pub bearer: Secret,
    pub expires_at: DateTime<Utc>,
    pub xsrf: String,
}

impl Tokens {
    /// Whether this token should be refreshed before the next request goes out.
    pub(crate) fn is_stale(&self) -> bool {
        Utc::now() + REFRESH_MARGIN >= self.expires_at
    }
}

/// The session, shared by every clone of a [`Client`](crate::Client).
#[derive(Debug)]
pub(crate) struct Session {
    cookies: Arc<CookieStoreMutex>,
    tokens: RwLock<Option<Tokens>>,
    file: Option<PathBuf>,
    /// Whether the cookies are still believed to identify the account.
    ///
    /// Cleared by a 401, which is what stops the client renewing a token from cookies the
    /// server has already stopped recognising: that renewal succeeds, hands back an
    /// anonymous token, and fails identically one request later.
    trusted: RwLock<bool>,
}

/// The on-disk form, written only when a session file was configured.
///
/// `version` exists so a later change to the shape can be ignored rather than
/// misread: an unreadable cache is a login, not an error.
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u8,
    xsrf: String,
    bearer: String,
    expires_at: DateTime<Utc>,
    /// cookie_store's own JSON, kept opaque so its format is its business.
    cookies: String,
}

const STORED_VERSION: u8 = 1;

impl Session {
    pub(crate) fn new(cookies: Arc<CookieStoreMutex>, file: Option<PathBuf>) -> Self {
        Self {
            cookies,
            tokens: RwLock::new(None),
            file,
            trusted: RwLock::new(true),
        }
    }

    /// The current tokens, if the client has signed in.
    pub(crate) async fn tokens(&self) -> Option<Tokens> {
        self.tokens.read().await.clone()
    }

    /// Install a fresh set of tokens and persist the session if a file was configured.
    pub(crate) async fn store(&self, tokens: Tokens) -> Result<()> {
        *self.tokens.write().await = Some(tokens);
        self.save().await
    }

    /// Forget the tokens, so the next request signs in again.
    ///
    /// The cookies stay: a 401 on a five-minute token usually means the token died, not
    /// the year-long cookie, and keeping the jar is what lets the refresh skip the
    /// password.
    pub(crate) async fn invalidate(&self) {
        *self.tokens.write().await = None;
        *self.trusted.write().await = false;
    }

    /// Whether a token may be renewed from the cookies instead of from the password.
    pub(crate) async fn cookies_trusted(&self) -> bool {
        *self.trusted.read().await
    }

    /// Record that a fresh login has made the cookies good again.
    pub(crate) async fn trust_cookies(&self) {
        *self.trusted.write().await = true;
    }

    /// Start the handshake from nothing, without disturbing the cache on disk.
    ///
    /// The file is left alone because a failed login should not cost the session that
    /// was working until a moment ago; a successful one overwrites it anyway.
    pub(crate) async fn reset(&self) {
        *self.tokens.write().await = None;
        lock(&self.cookies).clear();
    }

    /// Drop the whole session, cookies included, and remove the cache file.
    pub(crate) async fn clear(&self) -> Result<()> {
        *self.tokens.write().await = None;
        lock(&self.cookies).clear();
        if let Some(path) = &self.file {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(file_error(path, source)),
            }
        }
        Ok(())
    }

    /// Read a cached session from disk, if there is one worth reading.
    ///
    /// Anything unreadable, from the wrong version or already expired is treated as
    /// absent: the caller then signs in, which is what it would have done anyway.
    pub(crate) async fn restore(&self) -> bool {
        let Some(path) = &self.file else {
            return false;
        };
        let Ok(raw) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(stored) = serde_json::from_str::<Stored>(&raw) else {
            tracing::debug!(path = %path.display(), "ignoring an unreadable session cache");
            return false;
        };
        if stored.version != STORED_VERSION {
            return false;
        }
        let Ok(jar) = cookie_store::serde::json::load_all(Cursor::new(stored.cookies)) else {
            return false;
        };
        *lock(&self.cookies) = jar;
        *self.tokens.write().await = Some(Tokens {
            bearer: Secret::new(stored.bearer),
            expires_at: stored.expires_at,
            xsrf: stored.xsrf,
        });
        tracing::debug!(path = %path.display(), "restored a cached session");
        true
    }

    async fn save(&self) -> Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        let Some(tokens) = self.tokens().await else {
            return Ok(());
        };

        let mut cookies = Vec::new();
        // Including the session cookies is the point: `IVCookieBasketKey` has no expiry
        // and identifies the basket, so dropping it would hand the next run an empty one.
        cookie_store::serde::json::save_incl_expired_and_nonpersistent(
            &lock(&self.cookies),
            &mut cookies,
        )
        .map_err(|e| file_error(path, std::io::Error::other(e)))?;

        let stored = Stored {
            version: STORED_VERSION,
            xsrf: tokens.xsrf,
            bearer: tokens.bearer.expose().to_owned(),
            expires_at: tokens.expires_at,
            cookies: String::from_utf8_lossy(&cookies).into_owned(),
        };
        let json = serde_json::to_vec(&stored).map_err(|e| file_error(path, e.into()))?;

        write_private(path, &json).map_err(|source| file_error(path, source))
    }
}

/// Write a file only the owner can read, replacing whatever was there.
///
/// Via a temporary file in the same directory, so a crash mid-write leaves the previous
/// session intact rather than a truncated one. The mode is set before the rename, since
/// the window between creating a world-readable file and chmodding it is exactly long
/// enough to leak the year-long auth cookie.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;

    let tmp = path.with_extension("tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Take the cookie jar, recovering from a panic in another holder of the lock.
///
/// A poisoned cookie store is still a usable cookie store; the alternative is failing
/// every later request over a panic that already happened somewhere else.
fn lock(cookies: &CookieStoreMutex) -> std::sync::MutexGuard<'_, cookie_store::CookieStore> {
    match cookies.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn file_error(path: &Path, source: std::io::Error) -> Error {
    Error::SessionFile {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn jar() -> Arc<CookieStoreMutex> {
        Arc::new(CookieStoreMutex::new(cookie_store::CookieStore::default()))
    }

    fn tokens(expires_in: TimeDelta) -> Tokens {
        Tokens {
            bearer: Secret::new("jwt"),
            expires_at: Utc::now() + expires_in,
            xsrf: "xsrf".to_owned(),
        }
    }

    #[test]
    fn a_token_inside_the_refresh_margin_is_stale() {
        assert!(tokens(TimeDelta::seconds(5)).is_stale());
        assert!(!tokens(TimeDelta::seconds(300)).is_stale());
    }

    #[tokio::test]
    async fn a_session_without_a_file_persists_nothing() {
        let session = Session::new(jar(), None);
        session
            .store(tokens(TimeDelta::seconds(300)))
            .await
            .expect("storing without a file is a no-op");
        assert!(!session.restore().await);
        assert!(session.tokens().await.is_some());
    }

    #[tokio::test]
    async fn a_stored_session_comes_back_from_disk() {
        let dir = std::env::temp_dir().join(format!("nemlig-session-{}", std::process::id()));
        let path = dir.join("session.json");
        let _ = std::fs::remove_dir_all(&dir);

        let written = Session::new(jar(), Some(path.clone()));
        written
            .store(tokens(TimeDelta::seconds(300)))
            .await
            .expect("writes the cache");

        let read = Session::new(jar(), Some(path.clone()));
        assert!(read.restore().await);
        let restored = read.tokens().await.expect("tokens came back");
        assert_eq!(restored.xsrf, "xsrf");
        assert_eq!(restored.bearer.expose(), "jwt");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path)
                .expect("the file exists")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "the auth cookie must not be world-readable"
            );
        }

        read.clear().await.expect("removes the cache");
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_unreadable_cache_is_treated_as_no_cache() {
        let dir = std::env::temp_dir().join(format!("nemlig-garbage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creates the directory");
        let path = dir.join("session.json");
        std::fs::write(&path, b"not json").expect("writes garbage");

        let session = Session::new(jar(), Some(path));
        assert!(!session.restore().await);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
