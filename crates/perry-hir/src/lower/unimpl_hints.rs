//! Replacement-suggestion hints for the unimplemented-API gate (#925).
//!
//! Goal: when the #463 gate fires for a known-bad pattern that has a
//! direct supported equivalent, append a one-liner pointing at the
//! replacement. Without this, the error message is accurate ("X is not
//! implemented") but unhelpful — users have to grep through
//! `perry --print-api-manifest` (often >2 MB of output) to find the
//! Node-shaped surface Perry actually understands.
//!
//! Scope is intentionally narrow. Start with the two cases #925 calls
//! out by name; grow the table when other reports identify another
//! "compiled yesterday, doesn't today" pattern. Speculative entries are
//! not free: every wrong hint costs a user 30+ seconds of confusion, so
//! we only add entries we're confident about.
//!
//! Two distinct surfaces:
//!
//! 1. **Module-qualified unimplemented method/property** (e.g.
//!    `crypto.hmacSha256`). Looked up by `(module, prop)` via
//!    [`module_member_hint`]. The hint is appended to the existing
//!    `#463` error message at all three gate sites
//!    (`expr_member.rs::lower_member` for property reads,
//!    `expr_call.rs` for the 2-deep `mod.method()` and 3-deep
//!    `mod.Class.method()` call forms).
//!
//! 2. **Top-level `require("modname")`** (CJS-under-perry-compile, #668).
//!    Looked up by `modname` via [`require_module_hint`]. The hint is
//!    appended to the existing `Closes #668` error message in
//!    `expr_call.rs`.

/// Replacement hint for `module.prop` patterns that the #463 gate
/// rejects. Returns `None` if no specific hint is available — the
/// caller falls back to the generic "see `perry --print-api-manifest`"
/// pointer.
pub(crate) fn module_member_hint(module: &str, prop: &str) -> Option<&'static str> {
    // Normalize `node:crypto` and `crypto` to the same hint set; the
    // user typed whichever import shape they happened to copy off
    // StackOverflow and shouldn't have to care which one we keyed on.
    let m = strip_node_prefix(module);
    match (m, prop) {
        // #925 case 1: `crypto.hmacSha256(data, key)` was a Perry
        // shortcut in an earlier version. The Node-shaped chain Perry
        // recognizes today is `createHmac(algo, key).update(data).digest(enc)`.
        ("crypto", "hmacSha256") => Some(
            "Use `crypto.createHmac(\"sha256\", key).update(data).digest(\"hex\")` \
             instead (Perry recognizes this Node-shaped chain). For raw bytes, \
             omit the `\"hex\"` argument to `.digest()`.",
        ),
        _ => None,
    }
}

/// Replacement hint for `require("modname")` calls under
/// `perry compile`. Returns `None` if no specific hint is available —
/// the caller falls back to the generic
/// "use a static `import` instead" message.
///
/// The distinction this draws is: for a module that lives in Perry's
/// stdlib surface (`crypto`, `fs`, `path`, ...), swapping `require` for
/// a static `import` will Just Work, and the hint just nudges the user
/// toward the ESM form. For a module that ISN'T in Perry's stdlib at
/// all (e.g. `jose`), the static-import swap will compile but the
/// runtime call site will explode with `TypeError: value is not a
/// function` — so the hint warns explicitly about that.
pub(crate) fn require_module_hint(spec: &str) -> Option<String> {
    // Match the bare module name, ignoring any `node:` prefix the user
    // may have typed.
    let bare = strip_node_prefix(spec);
    match bare {
        // #925 case 2a: `require("crypto")` / `require("node:crypto")`.
        // The crypto stdlib surface exists, so ESM swap is enough.
        "crypto" => Some(format!(
            "`{spec}` is in Perry's stdlib — switch the `require` call to \
             a static ESM import: `import * as crypto from \"node:crypto\"` \
             (or named: `import {{ createHmac, randomUUID }} from \"node:crypto\"`)."
        )),
        // #925 case 2b: `require("jose")`. NOT in Perry's stdlib.
        // The static-import swap will compile (it's just an unresolved
        // import warning) but every method call will be a silent
        // undefined that blows up at runtime as
        // `TypeError: value is not a function` (#922). Warn explicitly.
        "jose" => Some(format!(
            "`{spec}` is not in Perry's stdlib — there is no shim for \
             `importX509` / `jwtVerify` / etc. Switching to a static \
             `import * as jose from \"jose\"` will compile, but every \
             method call will be `undefined` at runtime. \
             See `perry --print-api-manifest` for the supported surface."
        )),
        _ => None,
    }
}

/// Strip a leading `node:` prefix from a module specifier so the hint
/// table can key on the bare name without duplicating every entry.
fn strip_node_prefix(spec: &str) -> &str {
    spec.strip_prefix("node:").unwrap_or(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_hmac_sha256_has_hint() {
        let hint = module_member_hint("crypto", "hmacSha256").expect("hint exists");
        assert!(hint.contains("createHmac"));
        assert!(hint.contains("update"));
        assert!(hint.contains("digest"));
    }

    #[test]
    fn crypto_hmac_sha256_hint_normalizes_node_prefix() {
        // `node:crypto.hmacSha256` should route to the same hint.
        let hint = module_member_hint("node:crypto", "hmacSha256").expect("hint exists");
        assert!(hint.contains("createHmac"));
    }

    #[test]
    fn unknown_module_member_has_no_hint() {
        assert!(module_member_hint("fs", "doesNotExist").is_none());
        assert!(module_member_hint("nonexistent", "anything").is_none());
    }

    #[test]
    fn require_crypto_has_hint() {
        let hint = require_module_hint("crypto").expect("hint exists");
        assert!(hint.contains("stdlib"));
        assert!(hint.contains("import"));
        assert!(hint.contains("node:crypto"));
    }

    #[test]
    fn require_node_crypto_has_hint() {
        // `require("node:crypto")` should also be recognized.
        let hint = require_module_hint("node:crypto").expect("hint exists");
        assert!(hint.contains("import"));
    }

    #[test]
    fn require_jose_warns_about_runtime_undefined() {
        let hint = require_module_hint("jose").expect("hint exists");
        assert!(hint.contains("not in Perry"));
        assert!(hint.contains("undefined"));
    }

    #[test]
    fn require_unknown_module_has_no_hint() {
        assert!(require_module_hint("totally-fictional-pkg").is_none());
    }
}
