//! Regression test for the SECOND registration path in #10356.
//!
//! The first fix guarded the implicit import-walk loop, which registers every
//! exported class of a module an importer touches. That loop is not the only
//! way a class the importer never named gets bound under its bare name: the
//! transitive class closure in `run_pipeline` also pulls in whatever an
//! imported class's FIELD and RETURN types mention, so it can register a
//! shadowing class the first guard never sees.
//!
//! OpenCode's `packages/sdk/js/src/v2/gen/sdk.gen.ts` is exactly this shape:
//!
//! ```ts
//! export class OpencodeClient {
//!   private _request?: Request                  // field type
//!   get request(): Request { ... }              // getter return type
//! }
//! ```
//!
//! so `import { OpencodeClient }` registered `Request`, and
//! `new Request(url, init)` in the importer built the SDK's
//! `class Request extends HeyApiClient` rather than the global. The TUI died on
//! `next.headers.delete(...)` with "Cannot read properties of undefined".
//!
//! This is why the first fix passed every synthetic probe and still left
//! OpenCode broken: the probes' `OpencodeClient` had no member typed `Request`.
//! Add one and it reproduces in two modules.
//!
//! Parent refs are deliberately still registered — `class Sub extends Request`
//! needs its parent's layout or instances allocate too few inline slots
//! (#485). That exemption leaves a KNOWN remaining gap: importing a subclass
//! of a global-named class still binds the parent's name in the importer.
//! This test pins the field/return-type path only, which is the OpenCode case;
//! the parent-ref gap is tracked separately and deliberately NOT asserted here.

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

/// The shadowing class is reachable ONLY through `OpencodeClient`'s member
/// types — nothing in `main.ts` names it.
const SDK_SOURCE: &str = r#"
export class HeyApiClient { constructor(public c: any) {} }
export class Request extends HeyApiClient {
  readonly kind = "sdk-Request"
  list() { return [] }
}
export class OpencodeClient {
  readonly label = "OpencodeClient"
  private _request?: Request
  get request(): Request { return (this._request ??= new Request({ client: null })) }
}
"#;

const MAIN_SOURCE: &str = r#"
// Only OpencodeClient is imported. `Request` below must be the GLOBAL one --
// it reaches this module only via OpencodeClient's field and getter types.
import { OpencodeClient } from "./sdk.js"

const req: any = new Request("http://example.com/x", { method: "GET" })
console.log("1 method:", req.method)
console.log("2 url:", req.url)
console.log("3 typeof headers:", typeof req.headers)
console.log("4 leak-client:", "client" in req)
console.log("5 leak-kind:", req.kind)

// The OpenCode shape that failed: re-wrap, then touch headers.
const next: any = new Request(new URL("http://example.com/y"), req)
try {
  next.headers.delete("x-opencode-directory")
  console.log("6 headers.delete:", "ok")
} catch (e: any) {
  console.log("6 headers.delete:", "THREW " + e.message)
}

// The imported class itself still works, including the member typed Request.
const c = new OpencodeClient()
console.log("7 client label:", c.label)
console.log("8 client.request.kind:", (c.request as any).kind)
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "\
1 method: GET
2 url: http://example.com/x
3 typeof headers: object
4 leak-client: false
5 leak-kind: undefined
6 headers.delete: ok
7 client label: OpencodeClient
8 client.request.kind: sdk-Request
";

#[test]
fn closure_walk_does_not_shadow_a_global_intrinsic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("sdk.ts"), SDK_SOURCE).unwrap();
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
        "closure-walk probe must compile; stdout:\n{}\nstderr:\n{}",
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
        !stdout.contains("4 leak-client: true"),
        "a global Request must not carry HeyApiClient's `client` field; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("8 client.request.kind: sdk-Request"),
        "the imported class's own member typed `Request` must still resolve to \
         the SDK class -- the fix must not break the legitimate use; \
         stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "a class reachable only through an imported class's member TYPES must \
         not bind that name in the importing module"
    );
}
