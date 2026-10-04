//! The crate's error type.

use std::time::Duration;

use crate::types::InvalidValue;

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Anything that can go wrong talking to nemlig.com.
///
/// The variants are deliberately coarse. Prefer the predicate methods
/// ([`Error::is_not_found`] and friends) over matching, since the enum is
/// `#[non_exhaustive]` and gains variants as more of the API is covered.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request never produced a response: DNS, TLS, connect or timeout.
    #[error("{context}: {source}")]
    Transport {
        /// What was being attempted, without any query string.
        context: String,
        /// The underlying reqwest failure.
        #[source]
        source: reqwest::Error,
    },

    /// A URL could not be built from the configured base and the path segments.
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    /// The server answered with a status outside 2xx.
    #[error("{method} {path} returned {status}{}", Self::show_body(body))]
    Api {
        /// HTTP status code.
        status: u16,
        /// HTTP method of the request that failed.
        method: &'static str,
        /// Path of the request that failed, with no query string.
        path: String,
        /// The response body, truncated. nemlig.com has no error envelope, so this
        /// is whatever it sent: a JSON document, an HTML page, or nothing.
        body: String,
    },

    /// The response was 2xx but not the document this crate expected.
    #[error("could not read {expected} from the response: {source}")]
    Decode {
        /// What the crate was looking for, such as "the basket".
        expected: &'static str,
        /// The body it failed on, truncated.
        body: String,
        /// The serde failure.
        #[source]
        source: serde_json::Error,
    },

    /// A request body could not be serialized.
    #[error("could not encode the request body: {0}")]
    Encode(#[source] serde_json::Error),

    /// The credentials were rejected, or the session could not be established.
    #[error("{0}")]
    Auth(#[from] AuthError),

    /// The server asked for a pause longer than the client was willing to take.
    #[error("rate limited; the server asked for {}s", retry_after.as_secs())]
    RateLimited {
        /// What the server's `Retry-After` header asked for.
        retry_after: Duration,
    },

    /// The caller passed something the API would have rejected.
    #[error("{0}")]
    Invalid(#[from] InvalidValue),

    /// The session cache on disk could not be read or written.
    #[error("session file {path}: {source}")]
    SessionFile {
        /// The path that failed.
        path: String,
        /// The underlying I/O or serialization failure.
        #[source]
        source: std::io::Error,
    },

    /// The thing asked for is not in the account, or not in the catalogue.
    ///
    /// Raised by the lookups that search for something rather than addressing it
    /// directly, where the server answers 200 with nothing matching.
    #[error("no {kind} with id {id}")]
    NotFound {
        /// What was being looked for: "product", "order".
        kind: &'static str,
        /// The identifier that found nothing.
        id: String,
    },

    /// Order placement was compiled in but not enabled on this client.
    ///
    /// Only reachable with the `place-order` feature. See
    /// [`ClientBuilder::allow_place_order`](crate::ClientBuilder::allow_place_order).
    #[error("order placement is disabled; build the client with allow_place_order(true)")]
    PlaceOrderDisabled,
}

/// Why a login or a token refresh failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// nemlig.com accepted the request but did not sign the account in.
    ///
    /// A successful login answers with a `RedirectUrl`; anything else means the
    /// username or password was wrong, or the account needs attention on the website.
    #[error("nemlig.com did not sign the account in; check the username and password")]
    Rejected,

    /// A step of the login handshake returned a document without the field it needs.
    #[error("the {step} step did not return a {missing}")]
    Handshake {
        /// Which request: "anti-forgery", "token" or "login".
        step: &'static str,
        /// The field that was absent: "token" or "redirect".
        missing: &'static str,
    },
}

impl Error {
    /// The HTTP status, when the failure came from a response.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Whether the server said the thing does not exist.
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404) || matches!(self, Self::NotFound { .. })
    }

    /// Whether the session was rejected, by either status or the login handshake.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self.status(), Some(401 | 403)) || matches!(self, Self::Auth(_))
    }

    /// Whether the server asked the client to slow down.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited { .. }) || self.status() == Some(429)
    }

    /// Whether the failure is the server's fault and worth trying again.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Transport { .. }) || matches!(self.status(), Some(500..=599))
    }

    /// Build a transport error, naming the attempt without leaking the query string.
    ///
    /// `reqwest::Error`'s own `Display` embeds the full URL, and nemlig.com puts the
    /// account's user id in search query strings, so the message is built here instead
    /// of deriving `From<reqwest::Error>`.
    pub(crate) fn transport(context: impl Into<String>, source: reqwest::Error) -> Self {
        Self::Transport {
            context: context.into(),
            source: source.without_url(),
        }
    }

    fn show_body(body: &str) -> String {
        if body.is_empty() {
            String::new()
        } else {
            format!(": {body}")
        }
    }
}

/// How much of a response body is kept in an error message.
pub(crate) const MAX_BODY_IN_ERROR: usize = 512;

/// Shorten a body for an error message, cutting on a character boundary.
pub(crate) fn truncate(body: &str) -> String {
    if body.len() <= MAX_BODY_IN_ERROR {
        return body.trim().to_owned();
    }
    let mut end = MAX_BODY_IN_ERROR;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", body[..end].trim())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_body_shorter_than_the_limit_is_kept_whole() {
        assert_eq!(truncate("  hello  "), "hello");
    }

    #[test]
    fn a_long_body_is_cut_on_a_character_boundary() {
        let body = "æ".repeat(MAX_BODY_IN_ERROR);
        let cut = truncate(&body);
        assert!(cut.ends_with('…'));
        assert!(cut.is_char_boundary(cut.len() - '…'.len_utf8()));
    }

    #[test]
    fn a_rejected_login_counts_as_unauthorized() {
        let err = Error::Auth(AuthError::Rejected);
        assert!(err.is_unauthorized());
        assert!(!err.is_busy());
    }

    #[test]
    fn a_server_error_is_busy_but_not_unauthorized() {
        let err = Error::Api {
            status: 503,
            method: "GET",
            path: "/webapi/basket/GetBasket".to_owned(),
            body: String::new(),
        };
        assert!(err.is_busy());
        assert!(!err.is_unauthorized());
        assert_eq!(err.to_string(), "GET /webapi/basket/GetBasket returned 503");
    }
}
