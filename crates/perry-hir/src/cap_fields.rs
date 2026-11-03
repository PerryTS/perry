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

/// Codegen's `static_field_globals` key for slot `index` of a class's capture
/// environment (`Expr::ClassEnvGet`/`ClassEnvSet`). The leading `\u{1}` keeps
/// it disjoint from every real static field name.
pub fn class_env_slot_key(index: u32) -> String {
    format!("\u{1}perry_env{index}")
}

/// Codegen's `static_field_globals` key for a guarded class environment's
/// state global (`Expr::ClassEnvGet::guarded`).
pub fn class_env_state_key() -> String {
    "\u{1}perry_env_state".to_string()
}

/// The one predicate for "this name is a compiler capture param/member name":
/// exactly `__perry_cap_<digits>m<12 hex>` (what [`cap_field_name`] makes; the
/// runtime twin is `is_class_capture_key`). A user key such as
/// `__perry_cap_user` or `__perry_cap_7mine` is an ordinary property. Fields
/// are asked by origin ([`crate::ClassFieldOrigin::Capture`]); this is for
/// constructor parameters and member names, which have no origin.
pub fn is_cap_field_name(name: &str) -> bool {
    cap_field_outer_id(name).is_some()
}

/// Parse the outer local id from a cap param/field name: only the exact
/// salted spelling `__perry_cap_<id>m<12 hex>` has one.
pub fn cap_field_outer_id(name: &str) -> Option<u32> {
    let suffix = name.strip_prefix(CAP_FIELD_PREFIX)?;
    let end = suffix.bytes().position(|b| !b.is_ascii_digit())?;
    if end == 0 {
        return None;
    }
    let rest = suffix[end..].strip_prefix('m')?;
    if rest.len() != 12 || !rest.bytes().all(|b| b.is_ascii_hexdigit()) {
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
    fn unsalted_and_user_spellings_are_ordinary_names() {
        for n in [
            "__perry_cap_7",
            "__perry_cap_user",
            "__perry_cap_7mine",
            "__perry_cap_7m0000331fa6789",
            "__perry_cap_7m0000331fa67g",
            "__perry_cap_m0000331fa678",
        ] {
            assert_eq!(cap_field_outer_id(n), None, "{n}");
            assert!(!is_cap_field_name(n), "{n}");
        }
        assert!(is_cap_field_name(&cap_field_name(3, 7)));
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
