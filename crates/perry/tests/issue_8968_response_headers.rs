//! Regression for the two `Response` header defects found alongside #8968.
//! Both are silent wrong answers in the Web Fetch surface, not codegen bugs,
//! and neither is caused by the private-member miss the sibling test covers.
//!
//! 1. `js_response_new` took its `headers` parameter as "an f64 handle from
//!    `js_headers_new`". Codegen only produces one when it can SEE the header
//!    object — an inline `{ … }` literal or a local it tracked as an options
//!    object. For anything else (a `??` expression, a spread-built object, a
//!    call result, or the `headers` field read off a runtime options object) it
//!    passed the plain NaN-boxed JS value through, the registry lookup missed,
//!    and the response was built with NO headers.
//!
//! 2. Extracting a body from a STRING contributes
//!    `Content-Type: text/plain;charset=UTF-8` per the Fetch standard, and
//!    `new Response("hello")` did not set it.
//!
//! Together they cost hono the content type of every `c.text()` / `c.json()` /
//! `c.html()`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_source(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.ts");
    std::fs::write(&entry, source).expect("write source");
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output).output().expect("run binary");
    assert!(
        run.status.success(),
        "binary failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

/// Every spelling of a `headers` init that codegen cannot fold to a literal.
/// The inline-literal cases are the controls: they already worked, and must
/// keep working. Expected values are node 26 verbatim.
#[test]
fn response_headers_survive_a_non_literal_init() {
    let stdout = run_source(
        r#"function ct(r: Response): any { return r.headers.get("content-type"); }
function show(label: string, r: Response): void {
  console.log(label + " " + r.status + " " + JSON.stringify(ct(r)));
}
const literalLocal = { "content-type": "application/json" };
const spread: any = { "Content-Type": "application/json", ...(undefined as any) };
function fromCall(): any { return { "content-type": "application/json" }; }
show("inline", new Response("x", { headers: { "content-type": "application/json" } }));
function fromPairs(): any { return [["content-type", "application/x-pairs"]]; }
function fromCustomCall(): any { return { "content-type": "application/problem+json" }; }
show("local", new Response("x", { headers: literalLocal }));
show("spread", new Response("x", { headers: spread }));
show("coalesce", new Response("x", { headers: (undefined as any) ?? literalLocal }));
show("call", new Response("x", { headers: fromCall() }));
show("headers-obj", new Response("x", { headers: new Headers({ "content-type": "application/json" }) }));
show("pairs", new Response("x", { headers: fromPairs() }));
show("static-json-call", Response.json({ ok: true }, { headers: fromCustomCall() }));
const runtimeInit: any = { status: 201, headers: fromCall() };
show("runtime-init", new Response("x", runtimeInit));
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            r#"inline 200 "application/json""#,
            r#"local 200 "application/json""#,
            // `setDefaultContentType` in hono builds exactly this shape, and
            // with a capital `Content-Type` — the store lower-cases it.
            r#"spread 200 "application/json""#,
            r#"coalesce 200 "application/json""#,
            r#"call 200 "application/json""#,
            r#"headers-obj 200 "application/json""#,
            r#"pairs 200 "application/x-pairs""#,
            r#"static-json-call 200 "application/problem+json""#,
            r#"runtime-init 201 "application/json""#,
        ]
    );
}

/// Fetch §"extract a body": a string body contributes
/// `text/plain;charset=UTF-8` (that exact spelling, no space), and an explicit
/// header always wins over it. Body types that contribute nothing must keep
/// contributing nothing.
#[test]
fn string_body_contributes_the_default_content_type() {
    let stdout = run_source(
        r#"function show(label: string, r: Response): void {
  console.log(label + " " + JSON.stringify(r.headers.get("content-type")));
}
const nul: any = null;
show("string", new Response("hello"));
show("empty-init", new Response("hello", {}));
show("explicit-wins", new Response("hello", { headers: { "content-type": "application/json" } }));
show("bytes", new Response(new Uint8Array([1, 2, 3]) as any));
show("no-body", new Response());
function afterBodylessResponse(): any { new Response(); return {}; }
function afterBodiedResponse(): any { new Response("hello"); return {}; }
function afterRequest(): any { new Request("https://example.com", { method: "POST", body: "outer" }); return {}; }
show("nested-bodyless", new Response("outer", afterBodylessResponse()));
show("nested-bodied", new Response("outer", afterBodiedResponse()));
show("nested-request", new Response("outer", afterRequest()));
show("null-body", new Response(nul));
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            r#"string "text/plain;charset=UTF-8""#,
            r#"empty-init "text/plain;charset=UTF-8""#,
            r#"explicit-wins "application/json""#,
            "bytes null",
            "no-body null",
            r#"nested-bodyless "text/plain;charset=UTF-8""#,
            r#"nested-bodied "text/plain;charset=UTF-8""#,
            r#"nested-request "text/plain;charset=UTF-8""#,
            "null-body null",
        ]
    );
}
