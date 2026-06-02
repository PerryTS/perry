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

/// As of #561, the supported `crypto.subtle` surface (digest, importKey,
/// sign, verify) compiles. Reading the namespace itself also compiles
/// since `subtle` is registered as a property on `crypto` in the API
/// manifest.
#[test]
fn crypto_subtle_namespace_compiles() {
    let result = lower_result(
        r#"
        import * as crypto from "crypto";
        const ct = crypto.subtle;
    "#,
    );
    assert!(
        result.is_ok(),
        "crypto.subtle as a namespace read must compile (#561): {result:?}"
    );
}

/// `crypto.subtle.digest(...)` is supported as of #561.
#[test]
fn crypto_subtle_digest_compiles() {
    let result = lower_result(
        r#"
        import * as crypto from "crypto";
        async function go() {
          return await crypto.subtle.digest("SHA-256", new Uint8Array(8));
        }
    "#,
    );
    assert!(
        result.is_ok(),
        "crypto.subtle.digest(...) must compile (#561): {result:?}"
    );
}

/// crypto.subtle.encrypt now compiles after PR #952 added AES-GCM
/// support (and AES-CBC lowering through the same WebCryptoEncrypt
/// HIR variant). The pre-#952 "out of scope, must reject" assertion
/// has flipped — exercise the modern compile-success path so a
/// regression in the lowering would surface here.
#[test]
fn crypto_subtle_encrypt_compiles() {
    let result = lower_result(
        r#"
        import * as crypto from "crypto";
        async function go(k: any, d: any) {
          return await crypto.subtle.encrypt({ name: "AES-CBC" }, k, d);
        }
    "#,
    );
    assert!(
        result.is_ok(),
        "crypto.subtle.encrypt must compile after #952: {result:?}"
    );
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

#[test]
fn node_builtin_dispatch_only_named_imports_are_rejected() {
    let cases = [
        ("node:buffer", "alloc", "bufferAlloc"),
        ("node:buffer", "from", "bufferFrom"),
        ("node:crypto", "md5", "md5"),
        ("node:perf_hooks", "mark", "mark"),
        ("node:perf_hooks", "timeOrigin", "timeOrigin"),
        ("node:string_decoder", "encoding", "encoding"),
        ("node:tty", "clearLine", "clearLine"),
        ("node:process", "on", "processOn"),
        ("node:process", "setMaxListeners", "setMaxListeners"),
        ("node:module", "wrap", "moduleWrap"),
        ("module", "wrapper", "moduleWrapper"),
        ("node:url", "createObjectURL", "createObjectURL"),
        ("node:worker_threads", "getWorkerData", "getWorkerData"),
        ("node:https", "ClientRequest", "HttpsClientRequest"),
        ("node:http2", "Http2SecureServer", "Http2SecureServer"),
        ("node:child_process", "Stream", "ChildProcessStream"),
        ("node:cluster", "worker", "clusterWorker"),
        ("node:stream", "from", "streamFrom"),
        ("node:stream", "fromWeb", "streamFromWeb"),
    ];

    for (module, imported, local) in cases {
        let src = format!(
            r#"
            import {{ {imported} as {local} }} from "{module}";
            void {local};
        "#
        );
        let err = match lower_result(&src) {
            Ok(_) => panic!("{module}.{imported} should reject as a named import"),
            Err(err) => err,
        };
        assert!(
            err.contains("does not provide an export named")
                && err.contains(module)
                && err.contains(imported),
            "unexpected error for {module}.{imported}: {err}"
        );
    }
}

#[test]
fn node_builtin_real_named_exports_still_compile() {
    let result = lower_result(
        r#"
        import { Buffer, INSPECT_MAX_BYTES } from "node:buffer";
        import { randomUUID, hash } from "node:crypto";
        import { performance, PerformanceObserver } from "node:perf_hooks";
        import { StringDecoder } from "node:string_decoder";
        import { isatty, ReadStream, WriteStream } from "node:tty";
        import { cwd, env } from "node:process";
        import { builtinModules, createRequire } from "node:module";
        import { URL, fileURLToPath } from "node:url";
        import { Worker, workerData, parentPort } from "node:worker_threads";
        import { Agent, Server as HttpsServer } from "node:https";
        import { Http2ServerRequest, constants as http2Constants } from "node:http2";
        import { ChildProcess, spawn } from "node:child_process";
        import { Worker as ClusterWorker, fork } from "node:cluster";
        import { Readable, default as streamDefault } from "node:stream";

        console.log("ok");
    "#,
    );
    assert!(
        result.is_ok(),
        "real Node named exports must keep compiling: {result:?}"
    );
}

/// #3896: a bare value read of an unknown member on a Node-core module
/// namespace is an ordinary property miss → undefined (matching Node), but the
/// *call* form must still reject as unimplemented.
#[test]
fn crypto_unknown_method_is_rejected() {
    let read = lower_result(
        r#"
        import * as crypto from "crypto";
        const x = crypto.foobar;
    "#,
    );
    assert!(
        read.is_ok(),
        "crypto.foobar value read should resolve to undefined (#3896), got {read:?}"
    );
    let call = lower_result(
        r#"
        import * as crypto from "crypto";
        crypto.foobar();
    "#,
    );
    assert!(
        call.is_err(),
        "crypto.foobar() call should still error as unimplemented"
    );
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
        let is_core = perry_api_manifest::is_node_core_module(module);
        match lower_result(&src) {
            Ok(_) => {
                // #3896: a bogus member *value read* on a Node-core module
                // namespace is an ordinary property miss → undefined (matching
                // Node). The call form `m.bogus()` still rejects — see
                // `every_supported_module_rejects_bogus_call`. Only non-core
                // (npm) modules keep the strict read-gate.
                if !is_core {
                    failures.push(format!(
                        "{module}: bogus member access did not error (strict mode not engaged)"
                    ));
                }
            }
            Err(e) => {
                if is_core {
                    failures.push(format!(
                        "{module}: Node-core bogus member read should now resolve to undefined \
                         (#3896), not error — got {e}"
                    ));
                } else if !(e.contains("not implemented") && e.contains("#463")) {
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

/// Coverage sweep for #525: same as `every_supported_module_rejects_bogus_member`
/// but exercises the *call* form `m.bogus()` instead of the read form
/// `m.bogus`. Pre-#525, perry/* namespace modules (and a few others) bailed
/// in codegen's `lower_call/native.rs` with a per-module message (`'X' is
/// not a known function`) — different wording, different escape hatch
/// (consult `types/perry/<ns>/index.d.ts` instead of
/// `perry --print-api-manifest`), buried under a multi-level wrapping
/// chain. Now the canonical R005 message text fires uniformly across
/// every supported module regardless of which path the AST takes to
/// land at the rejection.
#[test]
fn every_supported_module_rejects_bogus_call() {
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

        let src = format!(
            r#"
            import * as m from "{module}";
            m.__perry_known_bogus_member_525__();
        "#
        );
        match lower_result(&src) {
            Ok(_) => {
                failures.push(format!(
                    "{module}: bogus call did not error (strict mode not engaged)"
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
        "{} module(s) failed the #525 R005 call-form sweep:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
