//! #503 — refuse compile-time dynamic dispatch on stdlib namespaces.
//!
//! Verifies the supply-chain-hardening HIR pass: `process[runtimeVar]()` and
//! similar shapes fail compilation, while user-side dynamic dispatch on user
//! objects, literal-keyed dispatch, and explicitly opted-out sites pass.

use perry_diagnostics::SourceCache;
use perry_hir::{
    clear_allow_dynamic_stdlib_packages, clear_current_module_source, lower_module,
    set_allow_dynamic_stdlib_packages, set_current_module_source,
    set_refuse_dynamic_stdlib_dispatch,
};
use perry_parser::parse_typescript_with_cache;

/// Try to lower `src`. Pin every relevant thread-local around the call so
/// test ordering can't poison subsequent tests (and so other tests in the
/// crate can't leak state into ours).
fn try_lower(src: &str, source_path: &str) -> Result<(), String> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, source_path, &mut cache)
        .map_err(|e| format!("parse failed: {}", e))?;
    set_refuse_dynamic_stdlib_dispatch(true);
    set_current_module_source(src.to_string());
    let outcome = lower_module(&parsed.module, "test", source_path);
    clear_current_module_source();
    clear_allow_dynamic_stdlib_packages();
    set_refuse_dynamic_stdlib_dispatch(true);
    outcome.map(|_| ()).map_err(|e| e.to_string())
}

#[test]
fn refuses_process_dynamic_dispatch() {
    let src = r#"
        const k: string = "exit";
        // @ts-ignore
        (process as any)[k](0);
    "#;
    let err = try_lower(src, "/tmp/host.ts").expect_err("should fail");
    assert!(
        err.contains("dynamic dispatch on stdlib namespace `process`"),
        "unexpected error: {err}"
    );
    assert!(err.contains("#503"), "error should cite issue: {err}");
}

#[test]
fn refuses_fs_dynamic_dispatch() {
    let src = r#"
        import fs from "fs";
        const name: string = "readFileSync";
        // @ts-ignore
        (fs as any)[name]("/tmp/x");
    "#;
    let err = try_lower(src, "/tmp/host.ts").expect_err("should fail");
    assert!(
        err.contains("dynamic dispatch on stdlib namespace `fs`"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_namespace_imported_alias() {
    let src = r#"
        import * as myFs from "fs";
        const m: string = "readFileSync";
        // @ts-ignore
        (myFs as any)[m]("/tmp");
    "#;
    let err = try_lower(src, "/tmp/host.ts").expect_err("should fail");
    assert!(
        err.contains("dynamic dispatch on stdlib namespace `fs`"),
        "unexpected error: {err}"
    );
}

#[test]
fn allows_literal_string_dispatch() {
    // `obj["literal"]` folds to PropertyGet → indistinguishable from
    // `obj.literal`. Never refused.
    let src = r#"
        // @ts-ignore
        const v = (process as any)["version"];
    "#;
    try_lower(src, "/tmp/host.ts").expect("literal-key dispatch must compile");
}

#[test]
fn allows_user_object_dispatch() {
    // The check is scoped to stdlib namespaces only. Dynamic dispatch on
    // user-defined identifiers must remain untouched — that is the
    // single most important non-regression of this pass.
    let src = r#"
        const me = { greet: (n: string) => "hi " + n };
        const k: string = "greet";
        // @ts-ignore
        const r = (me as any)[k]("world");
    "#;
    try_lower(src, "/tmp/host.ts").expect("user-object dispatch must compile");
}

#[test]
fn site_annotation_opts_out() {
    let src = r#"
        const k: string = "exit";
        // @perry-allow-dynamic
        // @ts-ignore
        (process as any)[k](0);
    "#;
    try_lower(src, "/tmp/host.ts").expect("site annotation must allow the call");
}

#[test]
fn site_annotation_same_line_opts_out() {
    // Same-line annotation is also recognised — covers the case where
    // the offending site is on a single line with a trailing comment.
    let src = r#"
        const k: string = "exit";
        // @ts-ignore
        (process as any)[k](0); // @perry-allow-dynamic
    "#;
    try_lower(src, "/tmp/host.ts").expect("trailing annotation must allow the call");
}

#[test]
fn per_package_allow_list_opts_out() {
    // Modules whose source path lies under `node_modules/<pkg>/` resolve
    // their owning package via `package_name_for_source_path`. If the
    // package is on the allow list, the check is skipped.
    let src = r#"
        const k: string = "exit";
        // @ts-ignore
        (process as any)[k](0);
    "#;
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "test.ts", &mut cache).unwrap();
    set_refuse_dynamic_stdlib_dispatch(true);
    let mut allow = std::collections::HashSet::new();
    allow.insert("legacy-dep".to_string());
    set_allow_dynamic_stdlib_packages(allow);
    set_current_module_source(src.to_string());

    // Path lives under node_modules/legacy-dep → covered by the allow list.
    let path = "/repo/node_modules/legacy-dep/src/index.ts";
    let outcome = lower_module(&parsed.module, "legacy-dep", path);
    clear_current_module_source();
    clear_allow_dynamic_stdlib_packages();
    outcome.expect("per-package allow list must opt the dep out");
}

#[test]
fn global_disable_opts_out() {
    let src = r#"
        const k: string = "exit";
        // @ts-ignore
        (process as any)[k](0);
    "#;
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "test.ts", &mut cache).unwrap();
    set_refuse_dynamic_stdlib_dispatch(false);
    set_current_module_source(src.to_string());
    let outcome = lower_module(&parsed.module, "test", "/tmp/host.ts");
    clear_current_module_source();
    // Re-arm the default so we don't poison subsequent tests on the same
    // thread (cargo may run multiple tests serially in this binary).
    set_refuse_dynamic_stdlib_dispatch(true);
    outcome.expect("disabling refusal must allow the call");
}

#[test]
fn site_annotation_ignored_inside_node_modules() {
    // #996 — a malicious dependency must not be able to grant itself
    // dynamic-dispatch permission by sitting a `// @perry-allow-dynamic`
    // next to its own call. Only the host (paths outside `node_modules/`)
    // may use the annotation; deps opt in via the host's
    // `perry.allowDynamicStdlibDispatch` list (or the global flag).
    let src = r#"
        const k: string = "exit";
        // @perry-allow-dynamic
        // @ts-ignore
        (process as any)[k](0);
    "#;
    let err = try_lower(src, "/repo/node_modules/evil/index.ts")
        .expect_err("annotation in node_modules must not opt out");
    assert!(
        err.contains("dynamic dispatch on stdlib namespace `process`"),
        "unexpected error: {err}"
    );
    // Host-code regression guard — same annotation in a host file must still pass.
    let host_outcome = try_lower(src, "/repo/src/main.ts");
    host_outcome.expect("annotation in host code must still opt out");
}

#[test]
fn package_name_extraction() {
    use perry_hir::package_name_for_source_path;
    assert_eq!(
        package_name_for_source_path("/repo/node_modules/lodash/lib/x.ts"),
        Some("lodash")
    );
    assert_eq!(
        package_name_for_source_path("/repo/node_modules/@scope/pkg/src/x.ts"),
        Some("@scope/pkg")
    );
    // Nested node_modules → rightmost wins (matches package resolution).
    assert_eq!(
        package_name_for_source_path("/repo/node_modules/outer/node_modules/inner/lib/x.ts"),
        Some("inner")
    );
    assert_eq!(package_name_for_source_path("/repo/src/main.ts"), None);
}

#[test]
fn diagnostic_names_the_namespace() {
    // The error message must name the offending namespace so reviewers
    // grepping CI logs can attribute the failure without reading the
    // surrounding source.
    let src = r#"
        import * as crypto from "crypto";
        const m: string = "randomBytes";
        // @ts-ignore
        (crypto as any)[m](16);
    "#;
    let err = try_lower(src, "/tmp/host.ts").expect_err("should fail");
    assert!(
        err.contains("`crypto`"),
        "diagnostic should name `crypto`: {err}"
    );
    assert!(
        err.contains("@perry-allow-dynamic"),
        "diagnostic should mention the opt-out: {err}"
    );
    assert!(
        err.contains("allowDynamicStdlibDispatch"),
        "diagnostic should mention the package.json key: {err}"
    );
}
