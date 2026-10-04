//! Turning an API failure into something a caller can act on.
//!
//! Every one of these is a tool error rather than a protocol error: the request was valid
//! and reached the tool, so the caller should see what went wrong. A protocol error is
//! rendered opaquely by most clients and the message never arrives.

use nemlig::Error;
use rmcp::model::{CallToolResult, ContentBlock};

/// What the call was addressing, so the message can name it.
///
/// The templates below never prepend an article, because some subjects are bare nouns
/// (`product 5070417`) and others are not (`the account's basket`).
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// What was addressed, such as `product 5070417`.
    pub what: &'a str,
    /// The tool that lists what does exist, when there is one.
    pub lister: Option<&'a str>,
}

impl<'a> Subject<'a> {
    /// A subject a caller could have misnamed, and the tool that would have named it right.
    pub fn new(what: &'a str, lister: &'a str) -> Self {
        Self {
            what,
            lister: Some(lister),
        }
    }

    /// For the tools that take no arguments, where nothing can be misnamed.
    pub fn account(what: &'a str) -> Self {
        Self { what, lister: None }
    }
}

/// Report a failed call, ending in what to do next.
pub fn failed(err: &Error, subject: Subject<'_>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message(err, subject))])
}

fn message(err: &Error, subject: Subject<'_>) -> String {
    let Subject { what, lister } = subject;
    let try_lister = |text: String| match lister {
        Some(lister) => format!("{text} {lister} finds what is there."),
        None => text,
    };

    if err.is_not_found() {
        return try_lister(format!("nemlig.com has no {what}."));
    }

    if err.is_unauthorized() {
        return "nemlig.com rejected the sign-in. Check NEMLIG_USERNAME and NEMLIG_PASSWORD \
                against the account on nemlig.com; every call fails until they are right."
            .to_owned();
    }

    if err.is_rate_limited() {
        return "nemlig.com is throttling this session and the client has already paced \
                itself and waited. It publishes no limit, so there is no number to work to; \
                the same call should succeed in about a minute."
            .to_owned();
    }

    if err.is_busy() {
        return format!("nemlig.com could not be reached or did not answer: {err}");
    }

    match err {
        Error::Decode { expected, .. } => format!(
            "nemlig.com answered with a body this server could not read as {expected}. The \
             website may have changed shape, which this client would need updating for."
        ),
        Error::Invalid(invalid) => format!("That argument will not do: {invalid}"),
        // Already carries the method, path, status and a truncated body, with no query string.
        other => other.to_string(),
    }
}

// There are no unit tests here: `nemlig::Error` is `#[non_exhaustive]`, so its variants
// cannot be built from outside the library. The mapping is covered end to end in
// `tests/read_tools.rs`, where a mocked server answers with the statuses that produce them.
