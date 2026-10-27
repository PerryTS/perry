//! In-memory `multipart/form-data` bodies (RFC 7578 over RFC 2046 framing).
//!
//! The boundary is derived from 16 bytes of entropy the host supplies (this crate
//! reads no randomness source) and is then checked against every part: a
//! boundary that occurs anywhere in a part's name, filename, content type or
//! bytes is replaced by the next candidate, so no payload - base64 text with a
//! boundary-shaped run included - can end a part early.
//!
//! ```
//! use turnloop_http::multipart::{Form, Part};
//! let form = Form::new()
//!     .text("version", "1.2.0")
//!     .part(Part::file("tarball", "pkg.tgz", b"\x1f\x8b...".to_vec()).content_type("application/gzip"));
//! let encoded = form.encode([7; 16]).unwrap();
//! assert!(encoded.content_type().starts_with("multipart/form-data; boundary="));
//! ```
use crate::{Error, Result, client::Request, http1::Header};

/// One form field: a text value or a named file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}
impl Part {
    /// A text field. No `Content-Type` is written unless one is set.
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            filename: None,
            content_type: None,
            body: value.into().into_bytes(),
        }
    }
    /// A file field. Its `Content-Type` is `application/octet-stream` unless set.
    pub fn file(name: impl Into<String>, filename: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            filename: Some(filename.into()),
            content_type: None,
            body: bytes,
        }
    }
    pub fn content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }
    fn media_type(&self) -> Option<&str> {
        match (&self.content_type, &self.filename) {
            (Some(content_type), _) => Some(content_type),
            (None, Some(_)) => Some("application/octet-stream"),
            (None, None) => None,
        }
    }
}

/// An ordered list of parts, encoded in one piece by [`Form::encode`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    parts: Vec<Part>,
}
impl Form {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn text(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.part(Part::text(name, value))
    }
    pub fn file(
        self,
        name: impl Into<String>,
        filename: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        self.part(Part::file(name, filename, bytes))
    }
    pub fn part(mut self, part: Part) -> Self {
        self.parts.push(part);
        self
    }
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }
    /// Serialize the form. `entropy` should come from the host's secure random
    /// source; any value yields a boundary absent from every part, but only
    /// unpredictable entropy keeps the boundary unguessable.
    ///
    /// A content type containing a control character (CR/LF included) is
    /// rejected. Quotes, CR and LF in names and filenames are percent-encoded as
    /// browsers do; other bytes are written as given.
    pub fn encode(&self, entropy: [u8; 16]) -> Result<Encoded> {
        for part in &self.parts {
            if part
                .media_type()
                .is_some_and(|t| t.bytes().any(|b| b.is_ascii_control()))
            {
                return Err(Error::new(
                    "UND_ERR_INVALID_ARG",
                    "invalid multipart content type",
                ));
            }
        }
        let names: Vec<_> = self
            .parts
            .iter()
            .map(|p| (escape(&p.name), p.filename.as_deref().map(escape)))
            .collect();
        let boundary = self.boundary(entropy, &names);
        // Framing per part is under 128 bytes beyond the boundary, name,
        // filename and content type; the closing delimiter adds 6.
        let mut body = Vec::with_capacity(
            self.parts
                .iter()
                .zip(&names)
                .map(|(part, (name, filename))| {
                    part.body.len()
                        + name.len()
                        + filename.as_ref().map_or(0, String::len)
                        + part.media_type().map_or(0, str::len)
                        + boundary.len()
                        + 128
                })
                .sum::<usize>()
                + boundary.len()
                + 6,
        );
        for (part, (name, filename)) in self.parts.iter().zip(&names) {
            body.extend_from_slice(b"--");
            body.extend_from_slice(boundary.as_bytes());
            body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
            body.extend_from_slice(name.as_bytes());
            body.push(b'"');
            if let Some(filename) = filename {
                body.extend_from_slice(b"; filename=\"");
                body.extend_from_slice(filename.as_bytes());
                body.push(b'"');
            }
            if let Some(media_type) = part.media_type() {
                body.extend_from_slice(b"\r\nContent-Type: ");
                body.extend_from_slice(media_type.as_bytes());
            }
            body.extend_from_slice(b"\r\n\r\n");
            body.extend_from_slice(&part.body);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(b"--\r\n");
        Ok(Encoded { boundary, body })
    }
    /// The first candidate that occurs in no part. Candidates are pairwise
    /// distinct and each one found in a part matches at least one of that
    /// part's finitely many substrings, so the search ends within
    /// (total part bytes + 1) candidates.
    fn boundary(&self, entropy: [u8; 16], names: &[(String, Option<String>)]) -> String {
        let entropy = u128::from_le_bytes(entropy);
        let (low, high) = (entropy as u64, (entropy >> 64) as u64);
        let mut index = 0u64;
        loop {
            let boundary = candidate(low, high, index);
            let needle = boundary.as_bytes();
            let collides = self
                .parts
                .iter()
                .zip(names)
                .any(|(part, (name, filename))| {
                    [
                        Some(part.body.as_slice()),
                        Some(name.as_bytes()),
                        filename.as_deref().map(str::as_bytes),
                        part.media_type().map(str::as_bytes),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|bytes| contains(bytes, needle))
                });
            if !collides {
                return boundary;
            }
            index += 1;
        }
    }
}

/// An encoded form body and the boundary its `Content-Type` must name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub boundary: String,
    pub body: Vec<u8>,
}
impl Encoded {
    /// The `Content-Type` header value for this body.
    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }
    /// Make this the request body, replacing any `Content-Type` header.
    pub fn apply(self, request: &mut Request) {
        let content_type = self.content_type();
        request
            .headers
            .retain(|h| !h.name.eq_ignore_ascii_case("content-type"));
        request
            .headers
            .push(Header::new("content-type", content_type));
        request.body = self.body;
    }
}

/// `"turnloop-"` and 32 hex digits: 41 characters, within RFC 2046's 70, and
/// no character that needs quoting in the `Content-Type` parameter.
const PREFIX: &str = "turnloop-";
const BOUNDARY_LEN: usize = PREFIX.len() + 32;

fn candidate(low: u64, high: u64, index: u64) -> String {
    // SplitMix64: its finalizer is a bijection and the odd increment makes each
    // index's input distinct, so every index yields a distinct first half.
    const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
    let first = mix(low.wrapping_add(index.wrapping_mul(GAMMA)));
    let second = mix(high ^ first);
    let mut boundary = String::with_capacity(BOUNDARY_LEN);
    boundary.push_str(PREFIX);
    for word in [first, second] {
        for shift in (0..16).rev() {
            let digit = ((word >> (shift * 4)) & 15) as u8;
            boundary.push(char::from(if digit < 10 {
                b'0' + digit
            } else {
                b'a' + digit - 10
            }));
        }
    }
    boundary
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The WHATWG form-data escaping of a name or filename.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => escaped.push_str("%22"),
            '\r' => escaped.push_str("%0D"),
            '\n' => escaped.push_str("%0A"),
            c => escaped.push(c),
        }
    }
    escaped
}
