//! A blocking HTTP/1.1 and WebSocket client on a self-owned `turnloop::Loop`.
//!
//! This exists so that Perry's **command-line driver** and the `perry-ext-*`
//! HTTP bindings can make requests without a `tokio::runtime::Runtime` and a
//! `reqwest::Client`. It is the P11 lane of the turnloop migration; see
//! `docs/turnloop/p11-report.md`.
//!
//! # Who may use this, and who may not
//!
//! Every other turnloop client in the tree is *completion-shaped*: it submits
//! work to the loop the JS agent already owns and settles a promise from a
//! sink. That is mandatory when a JS event loop is on the thread, and P5, P6
//! and P7 all paid for it. This crate is the opposite shape, and the
//! difference is the whole reason it can be so much smaller:
//!
//! **A caller here has no JS event loop to cooperate with.** It creates a loop,
//! turns it until the request finishes, and drops it. Two callers qualify:
//!
//! * the `perry` CLI, which compiles TypeScript and talks to registries — it
//!   has no JS agent at all;
//! * a `perry-ext-*` binding's request body, which already runs inside
//!   `perry_ffi::spawn_blocking` on a pool thread and used to block that thread
//!   on `tokio::runtime::Handle::current().block_on` in exactly the same place.
//!
//! Calling into this crate from a thread that owns a `turnloop::Loop` would
//! create a second loop on that thread, which is the mixed-transport deadlock
//! P1 had to work around (PerryTS/turnloop#45). Nothing in `perry-runtime` or
//! `perry-stdlib` depends on this crate, and nothing should.
//!
//! # What it is not
//!
//! No connection pool, no HTTP/2, no streaming response body, no cookie jar,
//! no automatic retry. Each of those is absent because no caller needs it, and
//! adding one would be adding an untested mode — the CLI makes a handful of
//! requests spread over minutes, and both bindings read the whole body before
//! settling their promise.
//!
//! # Example
//!
//! ```no_run
//! use perry_http_client::{Client, Request};
//! let client = Client::new();
//! let response = client.execute(Request::get("https://example.invalid/"))?;
//! assert!(response.is_success());
//! println!("{}", response.text());
//! # Ok::<(), perry_http_client::Error>(())
//! ```

pub mod http;
pub mod multipart;
pub mod tls;
pub mod transport;
pub mod ws;

use std::time::Duration;

pub use http::{BodySink, Options, Request};
pub use multipart::Form;
pub use turnloop_http::client::RedirectMode;
pub use ws::WebSocket;

/// Everything that can go wrong, flattened to a message.
///
/// The callers are CLI commands that print the error and exit, and two FFI
/// bindings that turn it into a JS string. Neither can act on a structured
/// variant, so carrying one would be scaffolding.
#[derive(Debug, Clone)]
pub struct Error {
    message: String,
    /// Set when the failure was a deadline rather than a refusal, because the
    /// CLI's polling loops treat those differently.
    timed_out: bool,
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            timed_out: false,
        }
    }

    pub(crate) fn other(message: impl Into<String>) -> Self {
        Self::new(message)
    }

    pub(crate) fn timeout(what: &str) -> Self {
        Self {
            message: format!("{what} timed out"),
            timed_out: true,
        }
    }

    /// Whether this failure was a deadline.
    pub fn is_timeout(&self) -> bool {
        self.timed_out
    }

    /// For a [`BodySink`] that refused a chunk — a disk write that failed, or
    /// a caller cancelling the transfer. `execute_streaming` stops feeding the
    /// sink and returns it.
    pub fn sink(message: impl Into<String>) -> Self {
        Self::new(message)
    }

    /// A transport failure. A deadline keeps its identity so the CLI's polling
    /// loops can tell "the server is slow" from "the server said no".
    pub(crate) fn io(what: &str, error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::TimedOut {
            return Self::timeout(what);
        }
        Self::new(format!("{what}: {error}"))
    }

    /// A protocol failure reported by `turnloop_http`, which carries a Node
    /// cause code (`ECONNREFUSED`, `UND_ERR_REDIRECT`, ...) worth keeping.
    pub(crate) fn protocol(what: &str, error: &turnloop_http::Error) -> Self {
        Self::new(format!("{what}: {} {}", error.code, error.message))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// One response, fully read.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
    /// The URL the response came from — after redirects, not the one asked for.
    pub url: String,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
    }

    /// `Err` with the status and body when the status is not 2xx —
    /// `reqwest::Response::error_for_status`'s shape, except that the body is
    /// already read so the message can carry it. Every CLI call site printed
    /// the status and then the body separately; this does both.
    pub fn error_for_status(self) -> Result<Self> {
        if self.is_success() {
            return Ok(self);
        }
        let body = self.text();
        let detail = body.trim();
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!(": {}", &detail[..detail.len().min(512)])
        };
        Err(Error::new(format!("HTTP {}{detail}", self.status)))
    }

    /// The body as UTF-8, replacing invalid sequences. Every caller here reads
    /// JSON or a human-readable error page, so a lossy decode is what they want
    /// and a `Result` would only be unwrapped.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// A configured client. Cheap to clone; holds no connection.
#[derive(Clone, Debug, Default)]
pub struct Client {
    options: Options,
}

impl Client {
    pub fn new() -> Self {
        Self::default()
    }

    /// A client whose whole-request budget is `timeout`.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            options: Options {
                timeout,
                ..Options::default()
            },
        }
    }

    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.options.timeout = timeout;
        self
    }

    #[must_use]
    pub fn redirect(mut self, mode: RedirectMode) -> Self {
        self.options.redirect = mode;
        self
    }

    #[must_use]
    pub fn user_agent(mut self, value: impl Into<String>) -> Self {
        self.options.user_agent = value.into();
        self
    }

    /// Turn off `HTTP_PROXY` / `HTTPS_PROXY` handling for this client.
    #[must_use]
    pub fn no_proxy(mut self) -> Self {
        self.options.use_proxy_environment = false;
        self
    }

    /// Raise the ceiling on a buffered response body (default 32 MiB).
    ///
    /// Only the two commands that download a build artifact into memory need
    /// this; everything else reads JSON and keeps the default. `reqwest` had no
    /// ceiling at all, so this is a new refusal — raising it at the call site
    /// rather than in the default is what keeps that refusal visible.
    #[must_use]
    pub fn max_body(mut self, bytes: usize) -> Self {
        self.options.max_body = bytes;
        self
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    /// Run one request to completion.
    pub fn execute(&self, request: Request) -> Result<Response> {
        http::execute(&self.options, request)
    }

    /// Run one request and hand the final response's body to `sink` as it
    /// arrives, instead of buffering it. GET and HEAD only.
    pub fn execute_streaming(&self, request: Request, sink: &mut dyn BodySink) -> Result<Response> {
        http::execute_streaming(&self.options, request, sink)
    }

    /// POST a `multipart/form-data` body.
    pub fn post_form(&self, url: impl AsRef<str>, form: Form) -> Result<Response> {
        let (content_type, body) = form.finish();
        self.execute(
            Request::post(url)
                .header("content-type", content_type)
                .body(body),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_response_reads_headers_case_insensitively() {
        let response = Response {
            status: 200,
            headers: vec![("Content-Type".into(), b"application/json".to_vec())],
            body: b"{}".to_vec(),
            url: "https://example.invalid/".into(),
        };
        assert_eq!(
            response.header("content-type"),
            Some(&b"application/json"[..])
        );
        assert_eq!(
            response.header("CONTENT-TYPE"),
            Some(&b"application/json"[..])
        );
        assert!(response.is_success());
    }

    #[test]
    fn a_request_lower_cases_its_header_names() {
        let request = Request::get("https://example.invalid/").header("X-Perry-Token", "abc");
        assert_eq!(request.headers[0].0, "x-perry-token");
    }

    #[test]
    fn error_for_status_carries_the_body() {
        let response = Response {
            status: 422,
            headers: Vec::new(),
            body: b"{\"error\":\"bad manifest\"}".to_vec(),
            url: "https://hub.invalid/api/v1/build".into(),
        };
        let error = response.error_for_status().expect_err("422 is not success");
        assert_eq!(error.to_string(), "HTTP 422: {\"error\":\"bad manifest\"}");
        assert!(!error.is_timeout());
    }

    #[test]
    fn error_for_status_passes_a_success_through() {
        let response = Response {
            status: 204,
            headers: Vec::new(),
            body: Vec::new(),
            url: "https://hub.invalid/".into(),
        };
        assert_eq!(response.error_for_status().expect("2xx passes").status, 204);
    }

    #[test]
    fn a_timeout_is_distinguishable_from_a_refusal() {
        assert!(Error::timeout("request").is_timeout());
        assert!(!Error::new("connection refused").is_timeout());
    }
}
