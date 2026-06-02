//! Opt-in compatibility reports (#849).
//!
//! When the compiler emits a diagnostic whose `DiagnosticCode` is one of the
//! "compatibility-gap" variants (`UnsupportedBinaryOp`, `UnsupportedExpression`,
//! `UnsupportedStatement`, `DynamicPropertyAccess`, `ImplicitCoercion`,
//! `UnresolvedImport`, `NoOpStub`), we may want to record an anonymous report
//! that points back at the unsupported feature so the maintainers can
//! prioritize fixes.
//!
//! This module is **the privacy boundary**:
//!
//! - We never construct a payload that contains raw source text — only a
//!   redacted snippet (identifiers anonymized to `<id1>`, `<id2>`; literals
//!   replaced by `<str>`/`<num>`; capped at 200 chars).
//! - We never send anything over the network from this crate. The actual
//!   upload is delegated to the CLI (`crates/perry/src/telemetry.rs`) via a
//!   `ReportSink` registered at process startup.
//! - If redaction fails any invariant (a quoted string survives, an
//!   upper-case identifier looks like leaked source, ...), the payload is
//!   rejected before it ever reaches the sink.
//!
//! Tests in `tests/redaction.rs` enforce the redaction guarantees with a
//! table-driven corpus.

use crate::diagnostic::DiagnosticCode;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

/// Hard cap on snippet length (post-redaction). The issue spec says 200.
pub const MAX_SNIPPET_LEN: usize = 200;

/// Compile pipeline stages a report can originate from. Mirrors the buckets
/// the maintainers use when triaging `known_failures.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportStage {
    HirLower,
    Codegen,
    Link,
    Runtime,
}

impl ReportStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportStage::HirLower => "hir-lower",
            ReportStage::Codegen => "codegen",
            ReportStage::Link => "link",
            ReportStage::Runtime => "runtime",
        }
    }
}

/// Maps to the `category` field of `test-parity/known_failures.json` so
/// user-reported gaps and maintainer-curated gaps are comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportCategory {
    /// A known TypeScript construct Perry doesn't handle yet (decorators,
    /// lookbehind regex, ...). Maps to `gap-categorical` in known_failures.
    GapCategorical,
    /// A bug discovered by bisection — likely a regression in a specific
    /// pass that used to work.
    GapBisect,
    /// Tracks an already-open issue.
    BugOpen,
    /// A previously-tracked bug that may still bite users.
    BugStale,
    /// Environment / CI plumbing problem, not a real compiler gap.
    CiEnv,
    /// Module inventory miss — a `node:*` API Perry doesn't carry.
    ModuleInventory,
}

impl ReportCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportCategory::GapCategorical => "gap-categorical",
            ReportCategory::GapBisect => "gap-bisect",
            ReportCategory::BugOpen => "bug-open",
            ReportCategory::BugStale => "bug-stale",
            ReportCategory::CiEnv => "ci-env",
            ReportCategory::ModuleInventory => "module-inventory",
        }
    }
}

/// The wire payload for a single compatibility report.
///
/// Only the redacted snippet ever leaves the machine. Field order is stable
/// because consumers may grep the JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityReport {
    /// `env!("CARGO_PKG_VERSION")` at the time the report was queued.
    pub perry_version: String,
    /// Anonymous client UUID — same one used by the generic telemetry channel.
    pub client_id: String,
    /// Stringified `DiagnosticCode` variant (e.g. `"UnsupportedExpression"`).
    pub code: String,
    /// `ReportCategory` variant as kebab-case string.
    pub category: String,
    /// `ReportStage` variant as kebab-case string.
    pub stage: String,
    /// `sha256:<hex>` of the redacted snippet, for server-side dedup.
    pub snippet_hash: String,
    /// Redacted, ≤200-char snippet of the offending source.
    pub snippet_redacted: String,
    /// Optional name of the TypeScript feature (e.g. `"decorator"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts_feature: Option<String>,
    /// Optional name of the Node API (e.g. `"node:async_hooks.createHook"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_api: Option<String>,
    /// `darwin-arm64`, `linux-x86_64`, ...
    pub os: String,
    /// `--target node:<n>` if available, else `"unknown"`.
    pub node_target: String,
}

impl CompatibilityReport {
    /// Build a report from raw fields. Performs redaction + snippet hash.
    ///
    /// Returns `Err(RedactionError)` if redaction fails an invariant — the
    /// caller MUST NOT fall back to the raw snippet on error.
    pub fn build(
        code: DiagnosticCode,
        category: ReportCategory,
        stage: ReportStage,
        raw_snippet: &str,
        client_id: String,
        perry_version: String,
        os: String,
        node_target: String,
        ts_feature: Option<String>,
        node_api: Option<String>,
    ) -> Result<Self, RedactionError> {
        let snippet_redacted = redact_snippet(raw_snippet)?;
        let snippet_hash = sha256_hash(&snippet_redacted);
        Ok(CompatibilityReport {
            perry_version,
            client_id,
            code: format!("{:?}", code),
            category: category.as_str().to_string(),
            stage: stage.as_str().to_string(),
            snippet_hash,
            snippet_redacted,
            ts_feature,
            node_api,
            os,
            node_target,
        })
    }
}

/// Why redaction refused to produce a payload. Each variant is something
/// the privacy invariant check caught — refuse, don't fall back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedactionError {
    /// A `"..."` or `'...'` quoted string survived redaction.
    QuotedStringSurvived,
    /// A template literal `` `...` `` survived redaction.
    TemplateLiteralSurvived,
    /// An upper-case identifier survived that isn't a known global (likely
    /// a user-defined class name leaking).
    UpperCaseIdentifierSurvived(String),
    /// Numeric literal survived after redaction.
    NumericLiteralSurvived,
    /// Input was empty or whitespace-only — no signal to report.
    EmptyAfterRedaction,
}

impl std::fmt::Display for RedactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RedactionError::QuotedStringSurvived => {
                write!(f, "redaction failed: a quoted string survived")
            }
            RedactionError::TemplateLiteralSurvived => {
                write!(f, "redaction failed: a template literal survived")
            }
            RedactionError::UpperCaseIdentifierSurvived(s) => {
                write!(f, "redaction failed: identifier `{}` survived", s)
            }
            RedactionError::NumericLiteralSurvived => {
                write!(f, "redaction failed: a numeric literal survived")
            }
            RedactionError::EmptyAfterRedaction => {
                write!(f, "redaction failed: snippet empty after redaction")
            }
        }
    }
}

impl std::error::Error for RedactionError {}

/// Globals that should NOT be anonymized to `<idN>` because they carry
/// signal — knowing the user wrote `Promise.race` is exactly the point.
/// Casing matters: `Promise` is preserved, but a user-defined `Foo` is not.
const PRESERVED_IDENTS: &[&str] = &[
    // JS built-ins
    "console",
    "Math",
    "JSON",
    "Promise",
    "Array",
    "Object",
    "String",
    "Number",
    "Boolean",
    "RegExp",
    "Date",
    "Error",
    "TypeError",
    "RangeError",
    "Symbol",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Reflect",
    "Proxy",
    "BigInt",
    "Buffer",
    "globalThis",
    "undefined",
    "null",
    "true",
    "false",
    "NaN",
    "Infinity",
    // Common Node globals
    "process",
    "require",
    "module",
    "exports",
    "__dirname",
    "__filename",
    "global",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "setImmediate",
    "clearImmediate",
    "queueMicrotask",
    "fetch",
    // TS/JS keywords (already lowercase mostly — listed for explicitness)
    "let",
    "const",
    "var",
    "function",
    "return",
    "if",
    "else",
    "for",
    "while",
    "do",
    "break",
    "continue",
    "switch",
    "case",
    "default",
    "try",
    "catch",
    "finally",
    "throw",
    "new",
    "delete",
    "typeof",
    "instanceof",
    "in",
    "of",
    "void",
    "this",
    "super",
    "class",
    "extends",
    "implements",
    "interface",
    "type",
    "enum",
    "namespace",
    "import",
    "from",
    "export",
    "as",
    "async",
    "await",
    "yield",
    "static",
    "public",
    "private",
    "protected",
    "readonly",
    "abstract",
    "declare",
    "any",
    "unknown",
    "never",
    "string",
    "number",
    "boolean",
    "object",
    "bigint",
    "symbol",
    "is",
];

/// Returns true if the identifier is a built-in we keep verbatim.
fn is_preserved_ident(s: &str) -> bool {
    PRESERVED_IDENTS.contains(&s)
}

/// Redact a raw source snippet according to the rules in #849:
///
/// 1. All string literals (single, double, backtick) → `"<str>"` / `` `<tpl>` ``
/// 2. All numeric literals → `<num>`
/// 3. All identifiers except [`PRESERVED_IDENTS`] → `<id1>`, `<id2>`, ... (stable across the snippet)
/// 4. Hard cap at 200 chars, truncate with `...`
/// 5. Reject if any invariant fails (no quoted string survives, no
///    obviously-user-named identifier survives).
///
/// This is a deliberately conservative tokenizer; it does not need to
/// understand TypeScript syntax — only enough to find literals and idents
/// reliably and replace them.
pub fn redact_snippet(input: &str) -> Result<String, RedactionError> {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut ident_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut next_id: usize = 1;

    while i < bytes.len() {
        let c = bytes[i];

        // Skip line comments
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Skip block comments
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }

        // String / template literals
        if c == b'"' || c == b'\'' {
            let quote = c;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                // Strings can't span newlines without escape — give up on this
                // string if we hit one (treat the rest as code).
                if bytes[i] == b'\n' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push_str("\"<str>\"");
            continue;
        }
        if c == b'`' {
            i += 1;
            // Template literals can be nested via `${...}` — we just scan
            // for the matching backtick and treat everything between as opaque.
            while i < bytes.len() && bytes[i] != b'`' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // consume closing `
            }
            out.push_str("`<tpl>`");
            continue;
        }

        // Numeric literal
        if c.is_ascii_digit() || (c == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            // Eat digits, dots, exponent markers, hex/oct/bin prefixes,
            // underscores, BigInt 'n' suffix
            while i < bytes.len() {
                let ch = bytes[i];
                if ch.is_ascii_digit()
                    || ch == b'.'
                    || ch == b'_'
                    || ch == b'x'
                    || ch == b'X'
                    || ch == b'o'
                    || ch == b'O'
                    || ch == b'b'
                    || ch == b'B'
                    || ch == b'e'
                    || ch == b'E'
                    || ch == b'n'
                    || (ch.is_ascii_hexdigit() && i > 0 && {
                        // Only inside a hex literal context (preceded by 0x...)
                        // — approximate by allowing hex digits after we've
                        // already started consuming. Safe overestimate.
                        true
                    })
                    || ((ch == b'+' || ch == b'-')
                        && i > 0
                        && (bytes[i - 1] == b'e' || bytes[i - 1] == b'E'))
                {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("<num>");
            continue;
        }

        // Identifier
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            while i < bytes.len() {
                let ch = bytes[i];
                if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'$' {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &input[start..i];
            if is_preserved_ident(ident) {
                out.push_str(ident);
            } else {
                let n = *ident_map.entry(ident.to_string()).or_insert_with(|| {
                    let n = next_id;
                    next_id += 1;
                    n
                });
                out.push_str(&format!("<id{}>", n));
            }
            continue;
        }

        // Punctuation / operator / whitespace — copy verbatim
        out.push(c as char);
        i += 1;
    }

    // Collapse multiple consecutive whitespace into a single space, drop leading/trailing
    let trimmed: String = {
        let mut t = String::with_capacity(out.len());
        let mut prev_ws = false;
        for ch in out.chars() {
            if ch.is_whitespace() {
                if !prev_ws && !t.is_empty() {
                    t.push(' ');
                }
                prev_ws = true;
            } else {
                t.push(ch);
                prev_ws = false;
            }
        }
        t.trim_end().to_string()
    };

    // Hard cap with ellipsis. Truncate on a char boundary so we don't
    // produce invalid UTF-8 mid-codepoint.
    let truncated = if trimmed.chars().count() > MAX_SNIPPET_LEN {
        let cap_chars = MAX_SNIPPET_LEN.saturating_sub(3);
        let mut s: String = trimmed.chars().take(cap_chars).collect();
        s.push_str("...");
        s
    } else {
        trimmed
    };

    if truncated.is_empty() {
        return Err(RedactionError::EmptyAfterRedaction);
    }

    // ---- Invariant checks ----------------------------------------------
    verify_redaction_invariants(&truncated)?;

    Ok(truncated)
}

/// Privacy invariants: anything that survives that should NOT have should
/// turn into an error so the caller refuses the report rather than sending
/// raw source.
fn verify_redaction_invariants(s: &str) -> Result<(), RedactionError> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];

        // Skip our own placeholders so they don't trip the invariant checks.
        // `<str>`, `<num>`, `<tpl>`, `<id1>`, `<id2>`, ...
        if c == b'<' {
            // Find matching '>'
            let close = s[i..].find('>').map(|p| i + p);
            if let Some(end) = close {
                i = end + 1;
                continue;
            }
        }

        // No quoted strings should survive outside of our `"<str>"` placeholder.
        // Our placeholder is `"<str>"`. Detect a quote whose interior isn't
        // exactly `<str>` or `<tpl>`.
        if c == b'"' {
            // Skip our own `"<str>"` placeholder.
            if s[i..].starts_with("\"<str>\"") {
                i += "\"<str>\"".len();
                continue;
            }
            return Err(RedactionError::QuotedStringSurvived);
        }
        if c == b'\'' {
            return Err(RedactionError::QuotedStringSurvived);
        }
        if c == b'`' {
            if s[i..].starts_with("`<tpl>`") {
                i += "`<tpl>`".len();
                continue;
            }
            return Err(RedactionError::TemplateLiteralSurvived);
        }

        // Identifier scan
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            while i < bytes.len() {
                let ch = bytes[i];
                if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'$' {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &s[start..i];
            // Pure-upper-case-and-digits like `MAX_LEN` is a user const leaking
            // — but `is_preserved_ident` will have allowed e.g. `JSON`, so we
            // only reject upper-case idents that are NOT in the preserved list.
            // Also: starts-with-upper-case (PascalCase) idents not in the
            // preserved list are most likely user types.
            if !is_preserved_ident(ident) {
                if let Some(first) = ident.chars().next() {
                    if first.is_ascii_uppercase() {
                        return Err(RedactionError::UpperCaseIdentifierSurvived(
                            ident.to_string(),
                        ));
                    }
                }
            }
            continue;
        }

        // Numeric literal leftover
        if c.is_ascii_digit() {
            return Err(RedactionError::NumericLiteralSurvived);
        }

        i += 1;
    }
    Ok(())
}

/// Compute `sha256:<hex>` of the input. Pure Rust, no external crate — we
/// can't pull `sha2` in for one hash. Small implementation lifted from
/// FIPS 180-4.
pub fn sha256_hash(input: &str) -> String {
    let digest = sha256(input.as_bytes());
    let mut s = String::from("sha256:");
    for b in digest {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Pre-processing: padding
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut buf: Vec<u8> = Vec::with_capacity(data.len() + 72);
    buf.extend_from_slice(data);
    buf.push(0x80);
    while buf.len() % 64 != 56 {
        buf.push(0);
    }
    buf.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in buf.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

/// Maps a `DiagnosticCode` to a default `ReportCategory`. Diagnostics that
/// aren't compatibility-relevant return `None` and won't generate reports.
pub fn category_for_code(code: DiagnosticCode) -> Option<ReportCategory> {
    use DiagnosticCode::*;
    match code {
        UnsupportedBinaryOp
        | UnsupportedExpression
        | UnsupportedStatement
        | UnsupportedPattern
        | UnsupportedFeature
        | UnsupportedPropertyKey
        | UnsupportedAssignmentTarget
        | UnsupportedCalleeType
        | UnsupportedUnaryOp
        | UnsupportedType => Some(ReportCategory::GapCategorical),
        DynamicPropertyAccess | ImplicitCoercion => Some(ReportCategory::GapCategorical),
        UnresolvedImport => Some(ReportCategory::ModuleInventory),
        NoOpStub | UnimplementedApi => Some(ReportCategory::ModuleInventory),
        // Codes we explicitly don't report on — too noisy or user-typo.
        _ => None,
    }
}

/// Returns true if the given `DiagnosticCode` is in the targeted set for
/// compatibility reporting.
pub fn is_reportable_code(code: DiagnosticCode) -> bool {
    category_for_code(code).is_some()
}

/// Description of a pending report that the chokepoint enqueues. The CLI
/// telemetry sink picks these up, applies the consent + dedup policy, and
/// either drops them or forwards them upstream.
#[derive(Debug, Clone)]
pub struct PendingReport {
    pub code: DiagnosticCode,
    pub category: ReportCategory,
    pub stage: ReportStage,
    pub raw_snippet: String,
    pub ts_feature: Option<String>,
    pub node_api: Option<String>,
}

/// Sink trait registered by the CLI at startup so `perry-diagnostics`
/// doesn't need to know about reqwest / config / threads.
pub trait ReportSink: Send + Sync {
    /// Called once per qualifying diagnostic emission. Implementations
    /// should be cheap — they typically just push onto a queue.
    fn submit(&self, pending: PendingReport);
}

fn sink_slot() -> &'static Mutex<Option<Box<dyn ReportSink>>> {
    static INSTANCE: OnceLock<Mutex<Option<Box<dyn ReportSink>>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(None))
}

/// Install a sink. Called once from the CLI at startup. Idempotent — a
/// later install replaces the previous sink (useful in tests).
pub fn set_report_sink(sink: Box<dyn ReportSink>) {
    if let Ok(mut guard) = sink_slot().lock() {
        *guard = Some(sink);
    }
}

/// Returns true if a sink has been installed. Lets the chokepoint short-
/// circuit when reporting is disabled at every layer.
pub fn has_report_sink() -> bool {
    sink_slot().lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Internal: forward a pending report to the installed sink, if any.
/// Called from the diagnostic emission chokepoint. No-op when no sink is
/// installed (which is the common case — only the `perry` CLI installs one).
pub fn enqueue_report(pending: PendingReport) {
    if let Ok(guard) = sink_slot().lock() {
        if let Some(sink) = guard.as_ref() {
            sink.submit(pending);
        }
    }
}

/// Convenience: given a built `Diagnostic` and a raw source snippet,
/// enqueue a report if the diagnostic's code is in the targeted set.
/// This is the single chokepoint referenced in the issue spec.
pub fn maybe_enqueue_for_diagnostic(
    code: DiagnosticCode,
    raw_snippet: &str,
    stage: ReportStage,
    ts_feature: Option<String>,
    node_api: Option<String>,
) {
    if !has_report_sink() {
        return;
    }
    let Some(category) = category_for_code(code) else {
        return;
    };
    if raw_snippet.trim().is_empty() {
        return;
    }
    enqueue_report(PendingReport {
        code,
        category,
        stage,
        raw_snippet: raw_snippet.to_string(),
        ts_feature,
        node_api,
    });
}

#[cfg(test)]
mod inline_tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        // "" -> e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hash(""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // "abc" -> ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hash("abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn redact_basic_identifier_renumbering() {
        let r = redact_snippet("let userName = otherName;").unwrap();
        assert!(r.contains("<id1>"));
        assert!(r.contains("<id2>"));
        assert!(!r.contains("userName"));
        assert!(!r.contains("otherName"));
    }
}
