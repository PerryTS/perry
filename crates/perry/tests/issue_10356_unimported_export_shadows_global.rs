//! Regression test for #10356: importing ONE name from a module also bound
//! that module's OTHER exported classes in the importer, so a user-defined
//! `class Request` shadowed the global fetch `Request`.
//!
//! `run_pipeline.rs` registers every exported class of every module an
//! importer touches, deliberately, "even when the class name wasn't in the
//! specifier list" — the stated safety argument being that a same-named LOCAL
//! class wins in `compile_module`. That argument holds for local classes, but
//! a global intrinsic is not a local class, so nothing outranked the implicit
//! entry: `new Request(url, init)` in the importer built the *exporter's*
//! class and `.headers` came back `undefined`.
//!
//! This was OpenCode's TUI bootstrap wall. `packages/sdk/js/src/v2/client.ts`
//! imports only `OpencodeClient` from `gen/sdk.gen.ts`, which also happens to
//! export `class Request extends HeyApiClient`; `rewrite()`'s
//! `new Request(url, request).headers.delete(...)` then threw
//! "Cannot read properties of undefined (reading 'delete')". Generated SDKs
//! exporting `Request`/`Response`/`Headers` are common (hey-api,
//! openapi-typescript, oazapfts).
//!
//! Per ESM a named import binds exactly the names it lists, so `Request` in
//! the importer is the global. bun, node and tsc all agree.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

/// Mirrors `packages/sdk/js/src/v2/gen/sdk.gen.ts`: a module that exports a
/// class named `Request` alongside the one class the importer actually wants.
const MOD_SOURCE: &str = r#"
export class HeyApiClient {
  client: any
  constructor(config: any) { this.client = config?.client ?? null }
}
export class Request extends HeyApiClient {
  readonly kind = "user-sdk-request"
}
export class Response {
  readonly kind = "user-sdk-response"
}
export class OpencodeClient {
  readonly name = "OpencodeClient"
}
"#;

/// A second module whose `Request` IS explicitly imported — the control. The
/// fix must not disturb a real named import of a global-shadowing class.
const EXPLICIT_SOURCE: &str = r#"
export class Request {
  readonly kind = "explicitly-imported"
  constructor(_a?: any, _b?: any) {}
}
"#;

/// `main.ts` imports only `OpencodeClient`, so every `Request` below is the
/// global one. `shadow.ts` is where an explicit import is exercised.
const MAIN_SOURCE: &str = r#"
import { OpencodeClient } from "./mod.js"
import { explicitKind } from "./shadow.js"

const c = new OpencodeClient()
console.log("1 import-works:", c.name)

const url = new URL("http://example.com/x?a=1")
const base = new Request(url, { method: "GET" })
console.log("2 base.method:", base.method)
console.log("3 base.url:", base.url)
console.log("4 typeof base.headers:", typeof base.headers)

// The exact OpenCode shape: re-wrap an existing Request, then touch .headers.
const next = new Request(url, base)
console.log("5 next.method:", next.method)
console.log("6 typeof next.headers:", typeof next.headers)
try {
  next.headers.delete("x-opencode-directory")
  console.log("7 headers.delete:", "ok")
} catch (e: any) {
  console.log("7 headers.delete:", "THREW " + e.message)
}
// The leak is visible as a field from a class that was never imported.
console.log("8 kind-leak:", (base as any).kind)

// `Response` is exported by mod.ts too and is likewise never imported.
console.log("9 response-status:", new Response("hi", { status: 201 }).status)
console.log("10 response-leak:", (new Response("hi") as any).kind)

// Control: an EXPLICITLY imported class of the same name must still win.
console.log("11 explicit-import:", explicitKind)
"#;

/// The explicit-import control lives in its own module so `main.ts` keeps the
/// global binding under test unpolluted.
const SHADOW_SOURCE: &str = r#"
import { Request } from "./explicit.js"
export const explicitKind = new Request("http://example.com/", {}).kind
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "\
1 import-works: OpencodeClient
2 base.method: GET
3 base.url: http://example.com/x?a=1
4 typeof base.headers: object
5 next.method: GET
6 typeof next.headers: object
7 headers.delete: ok
8 kind-leak: undefined
9 response-status: 201
10 response-leak: undefined
11 explicit-import: explicitly-imported
";

#[test]
fn unimported_export_does_not_shadow_a_global_intrinsic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mod.ts"), MOD_SOURCE).unwrap();
    std::fs::write(root.join("explicit.ts"), EXPLICIT_SOURCE).unwrap();
    std::fs::write(root.join("shadow.ts"), SHADOW_SOURCE).unwrap();
    std::fs::write(root.join("main.ts"), MAIN_SOURCE).unwrap();

    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "shadowing probe must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary must run; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8(run.stdout).expect("UTF-8 stdout");
    assert!(
        !stdout.contains("user-sdk-"),
        "no field of an un-imported class may appear on a global-intrinsic \
         instance; stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "a named import binds exactly the names it lists — the module's other \
         exports must not shadow globals in the importer"
    );
}
