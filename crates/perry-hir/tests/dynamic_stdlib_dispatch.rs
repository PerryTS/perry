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
fn allows_local_process_shadow_pipe_call() {
    let src = r#"
        const process = {
            pipe(value: string) {
                return value + "!";
            },
        };
        const out = process.pipe("ok");
    "#;
    try_lower(src, "/tmp/host.ts")
        .expect("local `process` binding must not lower as global process");
}

#[test]
fn allows_parameter_process_shadow_pipe_call() {
    let src = r#"
        function run(process: { pipe(value: string): string }) {
            return process.pipe("ok");
        }
    "#;
    try_lower(src, "/tmp/host.ts")
        .expect("parameter `process` binding must not lower as global process");
}

#[test]
fn allows_function_local_process_shadow_pipe_call() {
    let src = r#"
        interface Pipeable<T> {
            pipe<U>(fn: (value: T) => U): U
        }

        function makePipeable<T>(value: T): Pipeable<T> {
            return {
                pipe(fn) {
                    return fn(value)
                },
            }
        }

        function run(): string {
            const process = makePipeable("effect-pipe-shadow")
            return process.pipe((value) => `${value}: ok`)
        }
    "#;
    try_lower(src, "/tmp/host.ts")
        .expect("function-local `process` binding must not lower as global process");
}

#[test]
fn allows_class_process_shadow_pipe_call() {
    let src = r#"
        class process {
            static pipe(value: string) {
                return value;
            }
        }
        const out = process.pipe("ok");
    "#;
    try_lower(src, "/tmp/host.ts")
        .expect("class `process` binding must not lower as global process");
}

#[test]
fn allows_imported_process_shadow_pipe_call() {
    let src = r#"
        import { process } from "effect";
        const out = process.pipe("ok");
    "#;
    try_lower(src, "/tmp/host.ts")
        .expect("imported `process` binding must not lower as global process");
}

#[test]
fn still_refuses_global_process_pipe_call() {
    let src = r#"
        process.pipe("ok");
    "#;
    // #5245: the unimplemented-API gate only refuses at compile time in strict
    // mode; the default now defers `process.pipe` to a throw-on-reach value.
    perry_hir::set_unimplemented_strict_mode(true);
    let err = try_lower(src, "/tmp/host.ts").expect_err("global process.pipe must stay gated");
    perry_hir::set_unimplemented_strict_mode(false);
    assert!(
        err.contains("`process.pipe` is not implemented in Perry"),
        "unexpected error: {err}"
    );
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

/// #5263 — dynamic stdlib member access is allow-by-default (refusal off
/// unless lockdown / explicit opt-in). graceful-fs (`fs[symbolKey]`) and
/// fs-extra (`fs[method]`) shapes must lower cleanly with the default config.
/// Mirrors the compile driver, which leaves `refuse_dynamic_stdlib_dispatch`
/// false by default. The armed-gate behavior is covered by the `refuses_*`
/// tests above, which explicitly `set_refuse_dynamic_stdlib_dispatch(true)`.
#[test]
fn default_allows_dynamic_stdlib_member_access() {
    let src = r#"
        import * as fs from "node:fs";
        const k: string = "stat" + "Sync";
        // fs-extra-style dynamic method selection
        const m = (fs as any)[k];
        // graceful-fs-style symbol-keyed queue write + read
        const key = Symbol.for("graceful-fs.queue");
        (fs as any)[key] = [];
        const q = (fs as any)[key];
    "#;
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "test.ts", &mut cache).unwrap();
    // Default driver state: refusal OFF (#5263).
    set_refuse_dynamic_stdlib_dispatch(false);
    set_current_module_source(src.to_string());
    let outcome = lower_module(&parsed.module, "test", "/tmp/host.ts");
    clear_current_module_source();
    // Re-arm so we don't poison sibling tests on this thread.
    set_refuse_dynamic_stdlib_dispatch(true);
    outcome.expect("#5263: dynamic stdlib member access must lower by default");
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
fn allows_stdlib_subnamespace_static_method() {
    // #1723: `path[platform].matchesGlob(...)` — the dynamic key selects a
    // stdlib SUB-namespace (`path.win32` / `path.posix`) and the method is a
    // source-visible static ident. The method name is in plaintext, so this is
    // NOT the `ns[runtimeVar]()` obfuscation #503 targets (which HIDES the
    // method). Must compile. This is exactly Node's own `test-path-glob.js`
    // shape, the #800 node-core radar's only compile-fail.
    let src = r#"
        import * as path from "path";
        const platform: string = "win32";
        const r = path[platform].matchesGlob("foo/bar", "foo/*");
    "#;
    try_lower(src, "/tmp/host.ts").expect("auditable sub-namespace dispatch must compile");
}

#[test]
fn allows_stdlib_subnamespace_literal_key_method() {
    // `path[platform]["matchesGlob"](...)` — the terminal key is a string
    // literal (folds to a static property), so the method name is still
    // auditable. Allowed for the same reason as the dotted form.
    let src = r#"
        import * as path from "path";
        const platform: string = "posix";
        const r = path[platform]["matchesGlob"]("foo/bar", "foo/*");
    "#;
    try_lower(src, "/tmp/host.ts").expect("literal-key method on sub-namespace must compile");
}

#[test]
fn refuses_chained_dynamic_dispatch() {
    // `fs[a][b]` — BOTH keys dynamic, so the terminal member name is hidden:
    // the #1723 carve-out only covers a STATIC enclosing member, so this stays
    // refused as the obfuscation pattern.
    let src = r#"
        import fs from "fs";
        const a: string = "promises";
        const b: string = "readFile";
        // @ts-ignore
        (fs as any)[a][b]("/tmp/x");
    "#;
    let err = try_lower(src, "/tmp/host.ts").expect_err("chained dynamic dispatch must be refused");
    assert!(
        err.contains("dynamic dispatch on stdlib namespace `fs`"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_dynamic_key_inside_subnamespace_index() {
    // `path[fs[evil]].matchesGlob(...)` — the OUTER access is the auditable
    // `ns[dyn].static` shape, but the INDEX hides an `fs[runtimeVar]` dispatch.
    // The one-shot suppression must NOT leak into the index, so `fs[evil]` is
    // still refused (regression guard for the flag's scoping).
    let src = r#"
        import * as path from "path";
        import fs from "fs";
        const evil: string = "readFileSync";
        // @ts-ignore
        const r = path[(fs as any)[evil]].matchesGlob("a", "b");
    "#;
    let err = try_lower(src, "/tmp/host.ts").expect_err("dynamic key inside index must be refused");
    assert!(
        err.contains("dynamic dispatch on stdlib namespace `fs`"),
        "unexpected error: {err}"
    );
}

#[test]
fn refuses_terminal_dynamic_subselect() {
    // `const sub = path[platform];` — no static member follows, so the
    // dynamically-selected value is bound opaquely (you can't tell from source
    // what gets read off it later). Stays refused.
    let src = r#"
        import * as path from "path";
        const platform: string = "win32";
        // @ts-ignore
        const sub = (path as any)[platform];
    "#;
    let err =
        try_lower(src, "/tmp/host.ts").expect_err("terminal dynamic sub-select must be refused");
    assert!(
        err.contains("dynamic dispatch on stdlib namespace `path`"),
        "unexpected error: {err}"
    );
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
