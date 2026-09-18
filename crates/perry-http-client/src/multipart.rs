//! `multipart/form-data` bodies (RFC 7578).
//!
//! `turnloop_http::client` has no multipart builder — P8 named that as the one
//! thing blocking the CLI's uploads, because `perry publish`, `perry audit`,
//! `perry verify` and `perry run --remote` all posted a
//! `reqwest::multipart::Form`. This is the Perry-side answer while the HTTP
//! crate does not have one; the report says what turnloop should grow instead.
//!
//! Scope is deliberately exactly what those four commands send: text fields and
//! named file parts, built in memory. There is no streaming part, no
//! per-part charset and no nested `multipart/mixed` — Perry has never sent one,
//! and a builder that can produce shapes nobody tests is a liability.
//!
//! # Boundary
//!
//! RFC 2046 lets a boundary be up to 70 characters of a restricted set. This
//! builder uses `----perryFormBoundary` plus 32 hex characters, and — this is
//! the part that matters — it **verifies the boundary does not occur in any
//! part's bytes** before serializing, regenerating until it does not. A
//! boundary colliding with a base64 payload would silently truncate an upload,
//! and base64 is exactly what three of the four callers send.

use std::fmt::Write as _;

/// One field of a form.
enum Part {
    Text {
        name: String,
        value: Vec<u8>,
    },
    File {
        name: String,
        filename: String,
        content_type: String,
        bytes: Vec<u8>,
    },
}

/// An in-memory `multipart/form-data` body.
#[derive(Default)]
pub struct Form {
    parts: Vec<Part>,
}

impl Form {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a plain text field. `reqwest::multipart::Form::text`'s equivalent.
    #[must_use]
    pub fn text(mut self, name: &str, value: impl Into<Vec<u8>>) -> Self {
        self.parts.push(Part::Text {
            name: name.to_string(),
            value: value.into(),
        });
        self
    }

    /// Add a file part with an explicit filename and content type.
    #[must_use]
    pub fn file(
        mut self,
        name: &str,
        filename: &str,
        content_type: &str,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        self.parts.push(Part::File {
            name: name.to_string(),
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            bytes: bytes.into(),
        });
        self
    }

    /// Serialize to `(content_type_header_value, body)`.
    pub fn finish(self) -> (String, Vec<u8>) {
        let boundary = self.pick_boundary();
        let mut body = Vec::new();
        for part in &self.parts {
            body.extend_from_slice(b"--");
            body.extend_from_slice(boundary.as_bytes());
            body.extend_from_slice(b"\r\n");
            match part {
                Part::Text { name, value } => {
                    let mut head = String::new();
                    let _ = write!(
                        head,
                        "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                        escape(name)
                    );
                    body.extend_from_slice(head.as_bytes());
                    body.extend_from_slice(value);
                }
                Part::File {
                    name,
                    filename,
                    content_type,
                    bytes,
                } => {
                    let mut head = String::new();
                    let _ = write!(
                        head,
                        "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n\
                         Content-Type: {}\r\n\r\n",
                        escape(name),
                        escape(filename),
                        content_type
                    );
                    body.extend_from_slice(head.as_bytes());
                    body.extend_from_slice(bytes);
                }
            }
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(b"--\r\n");
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    /// A boundary that appears in no part's bytes.
    ///
    /// The probability of a collision with 128 bits of entropy is negligible,
    /// but "negligible" and "checked" are different claims, and a collision
    /// here corrupts an upload without any error. The check is a substring scan
    /// over the payload, which is linear and runs once.
    fn pick_boundary(&self) -> String {
        for attempt in 0..64u64 {
            let candidate = format!("----perryFormBoundary{:032x}", entropy(attempt));
            let needle = candidate.as_bytes();
            let collides = self.parts.iter().any(|part| match part {
                Part::Text { value, .. } => contains(value, needle),
                Part::File { bytes, .. } => contains(bytes, needle),
            });
            if !collides {
                return candidate;
            }
        }
        // 64 consecutive collisions against fresh entropy is not a case that
        // occurs; panicking beats emitting a body that would be truncated.
        panic!("could not find a multipart boundary absent from the payload");
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// 128 bits from the process's address-space layout, the clock and a counter.
///
/// This is not a CSPRNG and does not need to be: a multipart boundary is not a
/// secret, it only has to be absent from the payload, which `pick_boundary`
/// then verifies directly. Pulling `rand` into the CLI's link surface to
/// produce a delimiter would be the wrong trade.
fn entropy(attempt: u64) -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stack = &attempt as *const u64 as usize as u128;
    let mixed = nanos
        .wrapping_mul(0x2545_F491_4F6C_DD1D)
        .wrapping_add(stack.rotate_left(17))
        .wrapping_add(u128::from(attempt).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    mixed ^ (mixed >> 61)
}

/// RFC 7578 §5.1 takes the HTML5 escaping rules: a quote or a newline in a
/// field name would otherwise end the header early.
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(body: &[u8], boundary: &str) -> Vec<String> {
        String::from_utf8_lossy(body)
            .split(&format!("--{boundary}"))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn text_field_carries_its_name_and_value() {
        let (content_type, body) = Form::new().text("manifest", "{\"a\":1}").finish();
        let boundary = content_type
            .split("boundary=")
            .nth(1)
            .expect("boundary in content type")
            .to_string();
        let text = String::from_utf8(body).expect("ascii body");
        assert!(text.contains("Content-Disposition: form-data; name=\"manifest\"\r\n\r\n"));
        assert!(text.contains("{\"a\":1}"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn file_part_carries_filename_and_type() {
        let (content_type, body) = Form::new()
            .file("tarball", "app.tar.gz", "application/gzip", vec![1, 2, 3])
            .finish();
        let boundary = content_type.split("boundary=").nth(1).unwrap().to_string();
        let parts = split(&body, &boundary);
        assert_eq!(parts.len(), 3, "preamble, one part, terminator");
        assert!(parts[1].contains("filename=\"app.tar.gz\""));
        assert!(parts[1].contains("Content-Type: application/gzip"));
    }

    #[test]
    fn every_part_is_delimited_exactly_once() {
        let (content_type, body) = Form::new()
            .text("a", "1")
            .text("b", "2")
            .file("c", "c.bin", "application/octet-stream", vec![0u8; 16])
            .finish();
        let boundary = content_type.split("boundary=").nth(1).unwrap().to_string();
        let opens = body
            .windows(boundary.len() + 2)
            .filter(|w| w.starts_with(b"--") && &w[2..] == boundary.as_bytes())
            .count();
        // three parts plus the terminator
        assert_eq!(opens, 4);
    }

    /// The reason `pick_boundary` scans instead of trusting entropy. A payload
    /// that contains the boundary would truncate the upload at that point, and
    /// nothing downstream would report it.
    ///
    /// The check is on the boundary `finish` actually chose, not on a
    /// separately-generated one: `pick_boundary` draws fresh entropy per call,
    /// so comparing two calls would only be testing the entropy source.
    #[test]
    fn the_chosen_boundary_never_occurs_inside_a_part() {
        // A part built from a boundary of the same shape — the adversarial
        // case, and the one an attacker controlling an uploaded file could
        // construct.
        let planted = Form::new().text("probe", "").pick_boundary();
        let (content_type, body) = Form::new().text("payload", planted.clone()).finish();
        let chosen = content_type.split("boundary=").nth(1).unwrap().to_string();

        // Exactly two delimiters: the part's opener and the terminator. A
        // boundary that collided with the payload would make a third.
        let opens = body
            .windows(chosen.len() + 2)
            .filter(|w| w.starts_with(b"--") && &w[2..] == chosen.as_bytes())
            .count();
        assert_eq!(opens, 2, "chosen boundary must not occur inside a part");
        assert!(
            String::from_utf8_lossy(&body).contains(&planted),
            "the planted text must still be in the body, intact"
        );
    }

    #[test]
    fn a_quote_in_a_name_cannot_end_the_header() {
        let (_, body) = Form::new().text("we\"ird", "v").finish();
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("name=\"we%22ird\""));
    }

    #[test]
    fn an_empty_form_is_just_the_terminator() {
        let (content_type, body) = Form::new().finish();
        let boundary = content_type.split("boundary=").nth(1).unwrap().to_string();
        assert_eq!(body, format!("--{boundary}--\r\n").into_bytes());
    }
}
