//! Issue #463 — references to stdlib symbols Perry doesn't implement
//! must error at HIR lowering with a span pointing at the offending
//! property access. Companion to #464 (runtime first-call warnings on
//! intentional no-op stubs); this test covers the *not implemented at
//! all* case.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<perry_hir::Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// The motivating example from #463: `crypto.subtle` doesn't exist in
/// Perry's `crypto` surface, so accessing it must error.
#[test]
fn crypto_subtle_is_rejected() {
    let result = lower_result(
        r#"
        import * as crypto from "crypto";
        const ct = crypto.subtle;
    "#,
    );
    let err = result.expect_err("expected compile error for crypto.subtle");
    assert!(
        err.contains("crypto.subtle") && err.contains("not implemented"),
        "expected error naming `crypto.subtle` and `not implemented`, got: {err}"
    );
    // Pointer to the followup machinery / docs.
    assert!(
        err.contains("#463") || err.contains("--print-api-manifest"),
        "expected error to reference issue or escape hatch, got: {err}"
    );
}

/// `node:` prefix doesn't change the answer.
#[test]
fn node_prefix_subtle_is_rejected() {
    let result = lower_result(
        r#"
        import * as crypto from "node:crypto";
        const ct = crypto.subtle;
    "#,
    );
    assert!(result.is_err(), "node:crypto.subtle should error too");
}

/// Implemented `crypto` methods continue to compile.
#[test]
fn crypto_random_uuid_is_accepted() {
    let result = lower_result(
        r#"
        import * as crypto from "crypto";
        const id = crypto.randomUUID();
    "#,
    );
    assert!(
        result.is_ok(),
        "crypto.randomUUID() must continue to compile: {result:?}"
    );
}

/// Modules that have at least one manifest entry are strict — any
/// unknown property errors. `crypto.foobar` is not implemented.
#[test]
fn crypto_unknown_method_is_rejected() {
    let result = lower_result(
        r#"
        import * as crypto from "crypto";
        const x = crypto.foobar;
    "#,
    );
    assert!(result.is_err(), "crypto.foobar should error");
}

/// `os.platform` is implemented; `os.totalmem` too. Both must continue
/// to compile so the gap suite stays green.
#[test]
fn os_implemented_methods_compile() {
    let result = lower_result(
        r#"
        import * as os from "os";
        const p = os.platform;
        const m = os.totalmem;
    "#,
    );
    assert!(
        result.is_ok(),
        "os.platform / os.totalmem must continue to compile: {result:?}"
    );
}

/// Constants like `os.EOL` and `path.sep` are registered as Property
/// entries, not methods. They must compile.
#[test]
fn os_eol_and_path_sep_compile() {
    let result = lower_result(
        r#"
        import * as os from "os";
        import * as path from "path";
        const eol = os.EOL;
        const sep = path.sep;
    "#,
    );
    assert!(
        result.is_ok(),
        "os.EOL / path.sep must continue to compile: {result:?}"
    );
}

// PERRY_ALLOW_UNIMPLEMENTED=1 escape hatch is intentionally not unit-
// tested here: env vars are process-global and would race with the
// other tests' positive-error assertions in the same test binary.
// The escape is exercised end-to-end via `perry compile` integration
// (where each invocation is its own process) — see the README/docs
// added in this PR for the manual verification steps.

/// As of #513, every module in `NATIVE_MODULES` has at least one
/// manifest entry, so the permissive fall-through is unreachable for
/// supported modules. `axios.foo` (which used to silently compile under
/// the pre-#513 zero-entries-permissive shape) now errors.
///
/// The drift test `every_native_module_has_at_least_one_manifest_entry`
/// in `crates/perry-codegen/tests/manifest_consistency.rs` makes this
/// invariant load-bearing — adding a new native module without manifest
/// entries fails CI before the PR ships.
#[test]
fn supported_module_with_unknown_member_is_rejected() {
    let result = lower_result(
        r#"
        import axios from "axios";
        const x = axios.foo;
    "#,
    );
    let err = result.expect_err("axios.foo should error post-#513");
    assert!(
        err.contains("axios.foo") && err.contains("not implemented"),
        "expected error naming `axios.foo` and `not implemented`, got: {err}"
    );
}

/// Coverage sweep for #513: every module in `NATIVE_MODULES` must error
/// on a known-bogus property. Catches manifest entries that flag a
/// module as "covered" without actually flipping strictness on.
///
/// Side-effect-only sub-paths (`dotenv/config`) are skipped — they have
/// no value binding to read properties off, so the gate doesn't apply.
/// `tursodb` and `iroh` are external bindings (live in standalone
/// `@perryts/*` repos as of v0.5.557) — their manifest entries exist
/// but the in-tree resolver doesn't recognise them as a `NativeModuleRef`
/// without `node_modules/<pkg>/package.json` declaring `perry.nativeLibrary`,
/// so the gate's prerequisite shape never triggers in this isolated
/// HIR test.
#[test]
fn every_supported_module_rejects_bogus_member() {
    const SKIP: &[&str] = &[
        // Side-effect-only — no value binding to access.
        "dotenv/config",
        // External (non-bundled) bindings — out-of-tree as of v0.5.557.
        "tursodb",
        "iroh",
    ];

    let mut failures: Vec<String> = Vec::new();
    for &module in perry_api_manifest::NATIVE_MODULES {
        if SKIP.contains(&module) {
            continue;
        }
        // Sanity: prereq for the strict gate to fire.
        assert!(
            perry_api_manifest::module_has_any_entries(module),
            "module {module} has no entries — every_native_module_has_at_least_one_manifest_entry \
             should have caught this in perry-codegen"
        );

        // Pick an alias that's a valid TS identifier — `path` and
        // `events` are reserved-adjacent in some lints, but plain `m`
        // works for everything. Use namespace import so the binding
        // lowers to `Expr::NativeModuleRef`.
        let src = format!(
            r#"
            import * as m from "{module}";
            const x = m.__perry_known_bogus_member_513__;
        "#
        );
        match lower_result(&src) {
            Ok(_) => {
                failures.push(format!(
                    "{module}: bogus member access did not error (strict mode not engaged)"
                ));
            }
            Err(e) => {
                if !(e.contains("not implemented") && e.contains("#463")) {
                    failures.push(format!(
                        "{module}: errored but not via the R005/#463 path — got {e}"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} module(s) failed the R005 sweep:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
