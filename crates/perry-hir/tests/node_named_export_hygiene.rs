use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<perry_hir::Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                parse_typescript_with_cache(&src, "node_named_export_hygiene.ts", &mut cache)
                    .expect("parse should succeed");
            lower_module(&parsed.module, "test", "node_named_export_hygiene.ts")
                .map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn invalid_node_named_imports_are_rejected() {
    let cases = [
        (
            "buffer",
            r#"import { alloc } from "node:buffer"; console.log(alloc);"#,
        ),
        (
            "perf_hooks",
            r#"import { mark } from "node:perf_hooks"; console.log(mark);"#,
        ),
        (
            "string_decoder",
            r#"import { encoding } from "node:string_decoder"; console.log(encoding);"#,
        ),
        (
            "tty",
            r#"import { clearLine } from "node:tty"; console.log(clearLine);"#,
        ),
        (
            "process",
            r#"import { on, emit } from "node:process"; console.log(on, emit);"#,
        ),
        (
            "url",
            r#"import { createObjectURL } from "node:url"; console.log(createObjectURL);"#,
        ),
        (
            "worker_threads",
            r#"import { getWorkerData } from "node:worker_threads"; console.log(getWorkerData);"#,
        ),
        (
            "worker_threads",
            r#"import { postMessage } from "node:worker_threads"; console.log(postMessage);"#,
        ),
        (
            "https",
            r#"import { ClientRequest } from "node:https"; console.log(ClientRequest);"#,
        ),
        (
            "http2",
            r#"import { Http2SecureServer } from "node:http2"; console.log(Http2SecureServer);"#,
        ),
        (
            "http2 receiver methods",
            r#"import { listen, close, on, address } from "node:http2"; console.log("should-not-run", listen, close, on, address);"#,
        ),
        (
            "child_process",
            r#"import { Stream } from "node:child_process"; console.log(Stream);"#,
        ),
        (
            "cluster",
            r#"import { worker, on } from "node:cluster"; console.log(worker, on);"#,
        ),
        (
            "stream",
            r#"import { from, fromWeb, prototype } from "node:stream"; console.log(from, fromWeb, prototype);"#,
        ),
        (
            "crypto",
            r#"import { sha256, randomUUIDv7 } from "node:crypto"; console.log(sha256, randomUUIDv7);"#,
        ),
    ];

    let mut failures = Vec::new();
    for (module, src) in cases {
        match lower_result(src) {
            Ok(_) => failures.push(format!("{module}: invalid named import compiled")),
            Err(err) => {
                if !err.contains("does not provide an export named") {
                    failures.push(format!("{module}: unexpected error: {err}"));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} invalid import-shape case(s) failed:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn unknown_node_builtin_modules_are_rejected() {
    // #3925: a `node:`-prefixed specifier that isn't a real Node built-in must
    // be rejected (Node throws ERR_UNKNOWN_BUILTIN_MODULE). `punycode.ucs2` is
    // the representative phantom submodule — `ucs2` is a *property* of
    // `node:punycode`, not its own module — but it stays a Perry-internal
    // dispatch namespace, so this guards the import path specifically.
    let cases = [
        r#"import { decode, encode } from "node:punycode.ucs2"; console.log(decode, encode);"#,
        r#"import { foo } from "node:bogusmod"; console.log(foo);"#,
        r#"import "node:punycode.ucs2";"#,
    ];

    let mut failures = Vec::new();
    for src in cases {
        match lower_result(src) {
            Ok(_) => failures.push(format!("unknown builtin compiled: {src}")),
            Err(err) => {
                if !err.contains("No such built-in module") {
                    failures.push(format!("unexpected error for {src}: {err}"));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} unknown-builtin case(s) failed:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn valid_node_named_imports_keep_compiling() {
    let cases = [
        r#"import { Buffer, atob, isUtf8 } from "node:buffer"; console.log(Buffer, atob, isUtf8);"#,
        // #3925: real slash submodules must keep resolving past the new guard.
        r#"import { isMap, isDataView } from "node:util/types"; console.log(isMap, isDataView);"#,
        r#"import { readFile } from "node:fs/promises"; console.log(readFile);"#,
        r#"import { performance, PerformanceObserver, timerify } from "node:perf_hooks"; console.log(performance, PerformanceObserver, timerify);"#,
        r#"import { StringDecoder } from "node:string_decoder"; console.log(StringDecoder);"#,
        r#"import { isatty, ReadStream, WriteStream } from "node:tty"; console.log(isatty, ReadStream, WriteStream);"#,
        r#"import { cwd, env, version } from "node:process"; console.log(cwd, env, version);"#,
        r#"import { URL, fileURLToPath, domainToASCII } from "node:url"; console.log(URL, fileURLToPath, domainToASCII);"#,
        r#"import { Worker, workerData, isMainThread } from "node:worker_threads"; console.log(Worker, workerData, isMainThread);"#,
        r#"import { request, get, Agent, Server } from "node:https"; console.log(request, get, Agent, Server);"#,
        r#"import { createSecureServer, Http2ServerRequest, Http2ServerResponse, constants } from "node:http2"; console.log(createSecureServer, Http2ServerRequest, Http2ServerResponse, constants);"#,
        r#"import { exec, spawn, ChildProcess } from "node:child_process"; console.log(exec, spawn, ChildProcess);"#,
        r#"import { fork, Worker, workers } from "node:cluster"; console.log(fork, Worker, workers);"#,
        r#"import { default as pathDefault } from "node:path"; console.log(pathDefault.join("a", "b"));"#,
        r#"import { Readable, Writable, compose, default as streamDefault } from "node:stream"; console.log(Readable, Writable, compose, streamDefault);"#,
        r#"import { randomBytes, randomUUID, createHash } from "node:crypto"; console.log(randomBytes, randomUUID, createHash);"#,
    ];

    let mut failures = Vec::new();
    for src in cases {
        if let Err(err) = lower_result(src) {
            failures.push(err);
        }
    }

    assert!(
        failures.is_empty(),
        "{} valid import-shape case(s) failed:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn worker_threads_parent_port_call_keeps_property_call_shape() {
    let module = lower_result(
        r#"
        import * as workerThreads from "node:worker_threads";
        workerThreads.parentPort();
    "#,
    )
    .expect("parentPort() should lower as a normal call on the null property value");

    let debug = format!("{module:#?}");
    assert!(
        !debug.contains("method: \"parentPort\""),
        "parentPort() must not lower to the worker_threads native getter: {debug}"
    );
    assert!(
        debug.contains("Call")
            && debug.contains("\"worker_threads\"")
            && debug.contains("property: \"parentPort\""),
        "parentPort() should remain a normal call of worker_threads.parentPort: {debug}"
    );
}

#[test]
fn worker_threads_post_message_call_keeps_property_call_shape() {
    let module = lower_result(
        r#"
        import * as workerThreads from "node:worker_threads";
        workerThreads.postMessage("hello");
    "#,
    )
    .expect("postMessage() should lower as a normal call on the module property value");

    let debug = format!("{module:#?}");
    assert!(
        !debug.contains("method: \"postMessage\""),
        "module postMessage() must not lower to the worker_threads receiver method: {debug}"
    );
    assert!(
        debug.contains("Call")
            && debug.contains("\"worker_threads\"")
            && debug.contains("property: \"postMessage\""),
        "postMessage() should remain a normal call of worker_threads.postMessage: {debug}"
    );
}

#[test]
fn global_worker_messaging_constructors_lower_to_builtin_new() {
    // #4873: the *global* constructor forms (no worker_threads import) must
    // lower as `Expr::New` so codegen emits the always-linked runtime
    // constructors (`js_message_channel_new` / `js_broadcast_channel_new`).
    // Routing them to the worker_threads NativeMethodCall left an undefined
    // `js_worker_threads_message_channel_new` symbol in binaries that never
    // import `node:worker_threads` (React's scheduler hit this at init).
    let module = lower_result(
        r#"
        const a = new BroadcastChannel("a");
        const b = new globalThis.BroadcastChannel("b");
        const c = new MessageChannel();
        const d = new globalThis.MessageChannel();
        console.log(a, b, c, d);
    "#,
    )
    .expect("global messaging constructors should lower");

    let debug = format!("{module:#?}");
    assert!(
        !debug.contains("module: \"worker_threads\""),
        "global messaging constructors must not route to worker_threads-only symbols: {debug}"
    );
    let broadcast_count = debug.matches("class_name: \"BroadcastChannel\"").count();
    let message_channel_count = debug.matches("class_name: \"MessageChannel\"").count();
    assert!(
        broadcast_count >= 2 && message_channel_count >= 2,
        "global messaging constructors should lower as Expr::New on the builtin class: {debug}"
    );
}

#[test]
fn imported_worker_messaging_constructors_keep_worker_threads_routing() {
    // The genuinely-imported forms keep the stdlib NativeMethodCall routing —
    // the import anchors `js_worker_threads_*_new` in the link.
    let module = lower_result(
        r#"
        import { MessageChannel, BroadcastChannel } from "node:worker_threads";
        const a = new MessageChannel();
        const b = new BroadcastChannel("b");
        console.log(a, b);
    "#,
    )
    .expect("imported messaging constructors should lower");

    let debug = format!("{module:#?}");
    assert!(
        debug.contains("module: \"worker_threads\"")
            && debug.contains("method: \"MessageChannel\"")
            && debug.contains("method: \"BroadcastChannel\""),
        "imported messaging constructors should route through worker_threads native constructors: {debug}"
    );
}

/// A TS **type** name in a mixed import from a node-core module
/// (`import { createCipheriv, BinaryLike } from "crypto"` — `BinaryLike` is a
/// type, not a runtime export) must be ERASED, not rejected. tsc, esbuild, and
/// Bun all drop such a specifier; only Node's non-type-directed
/// `--experimental-strip-types` rejects it, and it demands an explicit
/// `import type`. Rejecting it failed the whole file to compile.
#[test]
fn type_only_names_in_a_mixed_import_are_erased_not_rejected() {
    let cases = [
        (
            "crypto",
            r#"
            import { createCipheriv, BinaryLike, CipherGCM } from "crypto";
            const key: BinaryLike = "0123456789abcdef";
            function f(c: CipherGCM) { return c; }
            console.log(typeof createCipheriv, key, typeof f);
        "#,
        ),
        (
            "stream",
            r#"
            import { Readable, WritableOptions } from "stream";
            function logCall(fn: WritableOptions["write"], id: number) { return id; }
            console.log(typeof Readable, logCall(undefined as any, 1));
        "#,
        ),
        (
            "type name never referenced at all",
            r#"
            import { readFileSync, PathLike } from "node:fs";
            console.log(typeof readFileSync);
        "#,
        ),
    ];

    for (label, src) in cases {
        assert!(
            lower_result(src).is_ok(),
            "{label}: a type-only name in a mixed import must be erased, not rejected"
        );
    }
}

/// The erasure above must NOT weaken the #3922 gate: a name that is not a value
/// export and IS referenced in a value position is a genuine bad import and must
/// still be rejected (Node throws a SyntaxError at module instantiation; binding
/// `undefined` would silently diverge).
#[test]
fn unknown_named_import_used_as_a_value_is_still_rejected() {
    let err = lower_result(
        r#"
        import { createCipheriv, definitelyNotAnExport } from "crypto";
        console.log(typeof createCipheriv);
        definitelyNotAnExport();
    "#,
    )
    .expect_err("a non-export used as a value must be rejected");
    assert!(
        err.contains("does not provide an export named 'definitelyNotAnExport'"),
        "expected the U006 export diagnostic, got: {err}"
    );
}
