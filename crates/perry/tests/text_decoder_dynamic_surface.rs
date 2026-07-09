//! Regression tests: TextDecoder / TextEncoder registry handles on
//! type-erased receivers.
//!
//! `new TextDecoder()` reached through a dynamic constructor value (e.g.
//! `new globalThis.TextDecoder` — the dynamic-construct path in
//! `class_registry/construct.rs`) returns a small registry-handle id. The
//! statically-typed `td.decode(buf)` call lowers straight to
//! `js_text_decoder_decode_llvm`, but every *dynamic* surface was missing:
//!
//! - property VALUE reads (`td.decode`, `td.encoding`) returned `undefined`
//!   through all three field-get funnels (`js_object_get_field_by_name`,
//!   its object tail, and the IC-miss handler);
//! - fused dynamic method calls (`td.decode(buf)` on an untyped local)
//!   fell through `js_native_call_method`'s field-scan and misbehaved.
//!
//! Canonical failure: a minified SDK's cached decodeText helper,
//!   `(PW7 ?? (K = new globalThis.TextDecoder, PW7 = K.decode.bind(K)))(q)`
//! — the `K.decode` read yielded a non-callable, so `.bind` threw
//! `TypeError: Bind must be called on a function`, killing every streamed
//! (SSE) API response in a large esbuild-bundled CLI app.
//!
//! Fix: `text_handle_property` (get_field_by_name_tail.rs) reifies the
//! method/accessor surface for VALUE reads at all three funnels, and a
//! dispatch arm in `native_call_method.rs` routes `decode` / `encode` /
//! `encodeInto` on text registry handles to the text natives.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The SDK decodeText shape: `K.decode.bind(K)` must yield a callable that
/// decodes (pre-fix: the read was `undefined` and `.bind` threw
/// "Bind must be called on a function").
#[test]
fn text_decoder_decode_bind_and_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const td: any = new (globalThis as any).TextDecoder();
console.log(typeof td.decode);
console.log(typeof td["decode"]);
const key = "dec" + "ode";
console.log(typeof td[key]);
const bound = td.decode.bind(td);
console.log(bound(new Uint8Array([104, 105])));
console.log(td.decode(new Uint8Array([104, 105])));
console.log(td.encoding, td.fatal, td.ignoreBOM);
"#,
    );
    assert_eq!(
        stdout,
        "function\nfunction\nfunction\nhi\nhi\nutf-8 false false\n"
    );
}

/// TextEncoder's dynamic surface: `enc.encode` read as a value + bound +
/// dynamic call, and `encodeInto`'s `{ read, written }` result.
#[test]
fn text_encoder_encode_bind_and_calls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const enc: any = new (globalThis as any).TextEncoder();
console.log(typeof enc.encode);
const bound = enc.encode.bind(enc);
const u8 = bound("hi");
console.log(u8 instanceof Uint8Array, u8[0], u8[1]);
const dest = new Uint8Array(8);
const r = enc.encodeInto("hi", dest);
console.log(r.read, r.written, dest[0], dest[1]);
"#,
    );
    assert_eq!(stdout, "function\ntrue 104 105\n2 2 104 105\n");
}

/// Decoder state must be honored through the dynamic surface: a latin1
/// decoder obtained dynamically decodes high bytes per its label.
#[test]
fn text_decoder_dynamic_label_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const td: any = new (globalThis as any).TextDecoder("latin1");
console.log(td.encoding);
const bound = td.decode.bind(td);
console.log(bound(new Uint8Array([0xe9])));
"#,
    );
    assert_eq!(stdout, "windows-1252\né\n");
}
