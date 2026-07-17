//! Class-capture instance-field naming.
//!
//! A class whose members reference enclosing-scope locals stashes those
//! captured values onto each instance as `this.__perry_cap_*` fields (written
//! by the constructor) and rebinds them in member prologues. The field name
//! must be unique **per defining module**, not just per capture id: local ids
//! restart at 0 in every module, and `super()` runs the parent constructor's
//! stashes on the SHARED instance. With bare `__perry_cap_<id>` names, a
//! derived-class method rebinding its own capture id K read the PARENT
//! MODULE's captured local K whenever the parent chain crossed modules and a
//! parent-side capture happened to share the id — Next.js's
//! `NextNodeServer.getBuildId` (next-server.js) rebound its `_fs` capture and
//! got base-server.js's `_interop_require_wildcard(require("path"))` instead:
//! `_fs.default.readFileSync(...)` dispatched on the `path` namespace,
//! returned undefined, and `.trim()` killed the server at boot.
//!
//! The name therefore embeds a stable per-module salt:
//! `__perry_cap_<id>m<salt-hex>`. Same-module inheritance keeps sharing
//! parent stashes (equal salt ⇒ equal names); cross-module chains are
//! isolated. Sites that need the outer id back (ctor-arg alignment, factory
//! specialization, capture writeback) parse the leading digits via
//! [`cap_field_outer_id`] instead of assuming the whole suffix is numeric.

/// Prefix shared by every class-capture field/param name.
pub const CAP_FIELD_PREFIX: &str = "__perry_cap_";

/// Instance-field / ctor-param name for a class-captured local. `salt` is the
/// defining module's stable salt (`LoweringContext::cap_salt`).
pub fn cap_field_name(salt: u64, id: u32) -> String {
    format!("{CAP_FIELD_PREFIX}{id}m{:012x}", salt & 0xFFFF_FFFF_FFFF)
}

/// Parse the outer local id from a cap field/param name. Accepts both the
/// salted `__perry_cap_<id>m<salt>` form and the legacy `__perry_cap_<id>`
/// (still produced by pre-salt HIR in caches/tests).
pub fn cap_field_outer_id(name: &str) -> Option<u32> {
    let suffix = name.strip_prefix(CAP_FIELD_PREFIX)?;
    let end = suffix
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(suffix.len());
    if end == 0 {
        return None;
    }
    suffix[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_roundtrip() {
        let n = cap_field_name(0xDEAD_BEEF_CAFE, 13);
        assert!(n.starts_with(CAP_FIELD_PREFIX));
        assert_eq!(cap_field_outer_id(&n), Some(13));
    }

    #[test]
    fn legacy_unsalted_parses() {
        assert_eq!(cap_field_outer_id("__perry_cap_7"), Some(7));
    }

    #[test]
    fn distinct_modules_distinct_names() {
        assert_ne!(cap_field_name(1, 13), cap_field_name(2, 13));
        assert_eq!(cap_field_name(9, 4), cap_field_name(9, 4));
    }

    #[test]
    fn non_cap_names_reject() {
        assert_eq!(cap_field_outer_id("__perry_capX"), None);
        assert_eq!(cap_field_outer_id("__perry_cap_"), None);
        assert_eq!(cap_field_outer_id("distDir"), None);
    }
}
