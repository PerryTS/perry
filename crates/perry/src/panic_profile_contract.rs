//! The panic-strategy contract for every profile that builds a runtime
//! archive Perry links into compiled programs (#7302).
//!
//! The exception transport requires the unwinder to step *through* runtime
//! Rust frames with `longjmp`-equivalent semantics. Under
//! `panic = "unwind"` rustc plants RFC-2945 abort-on-unwind guards in every
//! `extern "C"` function that contains an interior Rust call, so a JS throw
//! crossing such a helper — a throwing getter, a `JSON.parse` error, a
//! throwing `map` callback — aborts the process instead of being caught.
//!
//! This is not hypothetical and it is not caught by any other gate:
//! `[profile.dist]` (what `release-packages.yml` builds the SHIPPED
//! `libperry_{runtime,stdlib}.a` with) *inherits* `release` but then
//! re-declared `panic = "unwind"`, which wins. #7302 changed only
//! `[profile.release]`, so every local build, every CI job and the whole
//! parity suite were correct while the artifact users install would have
//! aborted on the first cross-helper throw. Nothing failed to compile;
//! nothing went red.
//!
//! So the invariant is asserted here, in `cargo-test` (per PR), by reading
//! the workspace manifest: a profile that ships a runtime must say
//! `panic = "abort"`, and `inherits` is not accepted as evidence because
//! the failure mode is precisely an override that looks harmless.

#[cfg(test)]
mod tests {
    /// Profiles used to build runtime archives that get linked into user
    /// programs. `perry-dev` inherits release and never re-declares panic,
    /// but it is listed so that a future re-declaration is caught too.
    const SHIPPING_PROFILES: &[&str] = &["release", "dist", "perry-dev"];

    fn workspace_manifest() -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("workspace manifest unreadable at {path}: {e}"))
    }

    /// The `panic = ...` value declared *directly* in `[profile.<name>]`,
    /// or `None` when the profile does not re-declare it (and therefore
    /// takes its parent's).
    fn declared_panic(manifest: &str, profile: &str) -> Option<String> {
        let header = format!("[profile.{profile}]");
        let start = manifest
            .find(&header)
            .unwrap_or_else(|| panic!("[profile.{profile}] not found in the workspace manifest"))
            + header.len();
        let body = &manifest[start..];
        // Stop at the next table header so a `[profile.X.package.Y]`
        // override block is not misread as part of this profile.
        let end = body.find("\n[").unwrap_or(body.len());
        for line in body[..end].lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("panic") {
                let rest = rest.trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    return Some(v.trim().trim_matches('"').to_string());
                }
            }
        }
        None
    }

    #[test]
    fn shipping_profiles_build_the_runtime_panic_abort() {
        let manifest = workspace_manifest();
        // The subject must be LIVE: if the parser stops finding the key it
        // is measuring nothing, and every profile would pass vacuously.
        assert_eq!(
            declared_panic(&manifest, "release").as_deref(),
            Some("abort"),
            "[profile.release] must declare panic = \"abort\" (see module docs)"
        );

        for profile in SHIPPING_PROFILES {
            match declared_panic(&manifest, profile) {
                // Re-declared: it must be abort. An `inherits` line does
                // NOT protect against this — that is exactly how the
                // shipped `dist` runtime ended up on the unwind strategy.
                Some(v) => assert_eq!(
                    v, "abort",
                    "[profile.{profile}] declares panic = \"{v}\"; a runtime built that way \
                     aborts the process on any JS throw that crosses an extern \"C\" helper \
                     with an interior Rust call (RFC 2945). See the module docs."
                ),
                // Not re-declared: inherits release, which the assertion
                // above pinned to abort.
                None => {}
            }
        }
    }

    /// The abort strategy is only half the contract: `panic = "abort"`
    /// omits unwind tables by default, and without them the unwinder cannot
    /// step runtime frames at all — every cross-helper throw is stranded
    /// rather than caught. The flag lives in `.cargo/config.toml` because a
    /// profile cannot carry rustflags.
    #[test]
    fn unwind_tables_are_forced_for_the_workspace() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.cargo/config.toml");
        let cfg = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cargo config unreadable at {path}: {e}"));
        let normalized = cfg.replace(['"', ' ', '\n'], "");
        assert!(
            normalized.contains("force-unwind-tables=yes"),
            ".cargo/config.toml must force unwind tables: panic=abort omits them, and the \
             exception transport cannot step runtime frames without them (the runtime \
             self-checks on the first `try` and aborts loudly). See the module docs."
        );
    }
}
