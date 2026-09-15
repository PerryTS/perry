//! Regression test for #10310: a `Headers` value must survive reaching a
//! DYNAMIC receiver when perry-ext-fetch owns the fetch surface.
//!
//! perry-ext-fetch wins the link for `js_headers_*` whenever the `node-fetch`
//! well-known binding — or its bare-`fetch` alias — routes there, and it keeps
//! its own handle store. It exported a strict SUBSET of perry-stdlib's surface,
//! so one `Headers` ended up half-owned by each crate, with different handle
//! encodings and separate registries:
//!
//! * the constructor returned the bare registry id rather than a NaN-boxed
//!   handle, so `typeof h` was `"number"` and `opts.headers.delete(k)` threw
//!   `(number).delete is not a function`; and
//! * `js_headers_init_from_value` came from perry-stdlib and read
//!   perry-stdlib's registry, so `new Headers(otherHeaders)` reported the init
//!   as non-iterable: `received 0x4014000000000000` — the double `5`.
//!
//! Statically-typed call sites HID both: `h.get(k)` lowers to a native call
//! taking the raw id, so it worked. Only dynamic dispatch and JS-level
//! reflection saw the number, which is why it surfaced as a TUI bootstrap
//! failure far from the cause (#10107).
//!
//! The binding routes on the SPECIFIER, and the binding replaces the package
//! source, so a stub `node_modules/node-fetch` is enough to select it — the
//! test needs no real dependency.

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

/// Every read goes through `o.headers`, an any-typed field, so the static
/// lowering cannot claim it and the dynamic handle tower must answer.
/// `new Headers(h)` covers the constructor half.
const SOURCE: &str = r#"
// @ts-nocheck
import fetch2 from "node-fetch"

const h: any = new Headers({ "Content-Type": "application/json", "X-Keep": "1" })
const o: any = { headers: h }

console.log("typeof", typeof o.headers)
o.headers.delete("Content-Type")
console.log("get gone", String(o.headers.get("Content-Type")))
console.log("get kept", String(o.headers.get("X-Keep")))
console.log("has", o.headers.has("X-Keep"))
let n = 0
o.headers.forEach(() => { n++ })
console.log("forEach", n)
console.log("second", String(new Headers(h).get("X-Keep")))
"#;

/// Byte-for-byte what bun prints.
///
/// `typeof fetch2` is deliberately NOT asserted: perry answers `"object"` where
/// bun answers `"function"` for node-fetch's default export. That is a separate,
/// pre-existing divergence and folding it in here would make this test fail for
/// an unrelated reason.
const EXPECTED: &str = "\
typeof object
get gone null
get kept 1
has true
forEach 1
second 1
";

#[test]
fn headers_survive_a_dynamic_receiver_under_the_fetch_binding() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), SOURCE).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "headers-probe", "version": "1.0.0", "type": "module" }"#,
    )
    .unwrap();

    // A stub is sufficient and intentional: the well-known binding routes on
    // the specifier and replaces the package source, so this selects
    // perry-ext-fetch without pulling a real dependency into the test.
    let stub = root.join("node_modules/node-fetch");
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::write(
        stub.join("package.json"),
        r#"{ "name": "node-fetch", "version": "3.3.2", "type": "module", "main": "index.js" }"#,
    )
    .unwrap();
    std::fs::write(stub.join("index.js"), "export default function fetch() {}\n").unwrap();

    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("--platform")
        .arg("bun")
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "the Headers probe must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("routing `node-fetch`"),
        "the test is only meaningful when perry-ext-fetch actually owns the \
         surface; the binding did not route:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    let stderr = String::from_utf8_lossy(&run.stderr).to_string();
    assert!(
        run.status.success(),
        "compiled binary must run; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("is not a function") && !stderr.contains("not iterable"),
        "no Headers operation may reach the number receiver again;\nstderr:\n{stderr}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "a Headers reached through an any-typed field must behave exactly as it \
         does through a typed one, and as bun does"
    );
}
