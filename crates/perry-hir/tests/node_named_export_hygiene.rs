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
fn valid_node_named_imports_keep_compiling() {
    let cases = [
        r#"import { Buffer, atob, isUtf8 } from "node:buffer"; console.log(Buffer, atob, isUtf8);"#,
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
