//! Regression for the Response header failures exposed by Hono in #8968.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn response_preserves_runtime_headers_and_string_content_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"function show(label: string, response: Response): void {
  console.log(label, response.status, JSON.stringify(response.headers.get("content-type")));
}
const spread: any = { "Content-Type": "application/json", ...(undefined as any) };
show("coalesce", new Response("x", { headers: (undefined as any) ?? spread }));
const init: any = { status: 404, headers: { "content-type": "application/json" } };
show("runtime-init", new Response("x", init));
show("headers", new Response("x", { headers: new Headers(spread) }));
show("default", new Response("x"));
show("explicit", new Response("x", { headers: { "content-type": "text/html" } }));
show("bytes", new Response(new Uint8Array([1, 2, 3]) as any));
show("empty", new Response());
function badInit(): any { throw new Error("boom"); }
try { new Response("must not leak", badInit()); } catch {}
show("after-throw", new Response());
"#,
    )
    .expect("write source");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(output).output().expect("run compiled program");
    assert!(
        run.status.success(),
        "program failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout)
            .lines()
            .collect::<Vec<_>>(),
        [
            r#"coalesce 200 "application/json""#,
            r#"runtime-init 404 "application/json""#,
            r#"headers 200 "application/json""#,
            r#"default 200 "text/plain;charset=UTF-8""#,
            r#"explicit 200 "text/html""#,
            "bytes 200 null",
            "empty 200 null",
            "after-throw 200 null",
        ]
    );
}
