//! End-to-end regression test for #5234: a static `.wasm` ESM import is
//! embedded and instantiated during module initialization, and its exports are
//! available through namespace, named, and default import forms.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

fn ensure_runtime_archives() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let runtime_build = Command::new(&cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("--features")
            .arg("perry-runtime/wasm-host")
            .output()
            .expect("build static runtime wrapper with wasm host shims");
        assert!(
            runtime_build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&runtime_build.stdout),
            String::from_utf8_lossy(&runtime_build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archives();
    target_debug_dir()
}

const ADD_WASM_BASE64: &str = "AGFzbQEAAAABBwFgAn9/AX8DAgEABwcBA2FkZAAACgkBBwAgACABags=";

/// `(module
///    (import "./glue" "inc" (func $inc (param i32) (result i32)))
///    (func (export "call") (param i32) (result i32)
///      local.get 0
///      call $inc))`
const IMPORTED_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01, 0x7f,
    0x02, 0x0e, 0x01, 0x06, 0x2e, 0x2f, 0x67, 0x6c, 0x75, 0x65, 0x03, 0x69, 0x6e, 0x63, 0x00, 0x00,
    0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x63, 0x61, 0x6c, 0x6c, 0x00, 0x01, 0x0a, 0x08,
    0x01, 0x06, 0x00, 0x20, 0x00, 0x10, 0x00, 0x0b,
];

/// `(module (memory (export "memory") 1))`
const MEMORY_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x0a, 0x01,
    0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00,
];

/// A module with live memory reads/writes and a wasm-bindgen-style
/// multi-value result `(i32, f64)`.
const LIVE_MEMORY_MULTI_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0f, 0x03, 0x60, 0x00, 0x01, 0x7f, 0x60,
    0x01, 0x7f, 0x00, 0x60, 0x01, 0x7c, 0x02, 0x7f, 0x7c, 0x03, 0x04, 0x03, 0x00, 0x01, 0x02, 0x05,
    0x03, 0x01, 0x00, 0x01, 0x07, 0x20, 0x04, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00,
    0x04, 0x6c, 0x6f, 0x61, 0x64, 0x00, 0x00, 0x05, 0x73, 0x74, 0x6f, 0x72, 0x65, 0x00, 0x01, 0x04,
    0x70, 0x61, 0x69, 0x72, 0x00, 0x02, 0x0a, 0x1a, 0x03, 0x07, 0x00, 0x41, 0x00, 0x2d, 0x00, 0x00,
    0x0b, 0x09, 0x00, 0x41, 0x00, 0x20, 0x00, 0x3a, 0x00, 0x00, 0x0b, 0x06, 0x00, 0x41, 0x07, 0x20,
    0x00, 0x0b,
];

/// `(module (table (export "refs") 2 externref))`
const EXTERNREF_TABLE_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x04, 0x04, 0x01, 0x6f, 0x00, 0x02, 0x07, 0x08,
    0x01, 0x04, 0x72, 0x65, 0x66, 0x73, 0x01, 0x00,
];

/// A wasm-bindgen-style cycle: the exported function calls JS glue which
/// mutates the module's exported externref table while Wasm is on the stack.
const EXTERNREF_TABLE_CYCLE_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00, 0x02, 0x15,
    0x01, 0x0c, 0x2e, 0x2f, 0x74, 0x61, 0x62, 0x6c, 0x65, 0x2d, 0x67, 0x6c, 0x75, 0x65, 0x04, 0x69,
    0x6e, 0x69, 0x74, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00, 0x04, 0x04, 0x01, 0x6f, 0x00, 0x02, 0x07,
    0x10, 0x02, 0x04, 0x72, 0x65, 0x66, 0x73, 0x01, 0x00, 0x05, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00,
    0x01, 0x0a, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0b,
];

const MAIN_FIXTURE: &str = r#"
import wasmDefault, { add } from "./add.wasm";
import * as wasmNamespace from "./add.wasm";
import { call as importedCall } from "./imported.wasm";
import { throughGlue } from "./glue";
import memoryDefault, { memory } from "./memory.wasm";
import liveDefault, { load, store, pair, memory as liveMemory } from "./live-memory.wasm";
import { refs } from "./externref-table.wasm";
import { runTableCycle } from "./table-glue";
import addWasmPath from "./file-add.wasm" with { type: "file" };
import importedWasmPath from "./file-imported.wasm" with { type: "file" };
import { readFileSync } from "node:fs";

console.log("namespace=" + wasmNamespace.add(2, 3));
console.log("named=" + add(7, 8));
console.log("default=" + wasmDefault.add(20, 22));
console.log("imported=" + importedCall(41));
console.log("circular=" + throughGlue(8));
console.log("memory=" + memory.buffer.byteLength);
console.log("defaultMemory=" + memoryDefault.memory.buffer.byteLength);

// wasm-bindgen writes arguments through memory.buffer before calling an
// export, then reads returned bytes after the call. Both directions must stay
// coherent with the host engine, and multi-value returns must remain arrays.
new Uint8Array(liveMemory.buffer)[0] = 42;
console.log("liveLoad=" + load());
store(99);
console.log("liveStore=" + new Uint8Array(liveDefault.memory.buffer)[0]);
const pairResult = pair(3);
console.log("multi=" + pairResult[0] + ":" + pairResult[1]);
const oldTableLength = refs.grow(3);
refs.set(4, true);
console.log("table=" + oldTableLength + ":" + refs.length + ":" + refs.get(4));
console.log("tableCycle=" + runTableCycle());

// #8508: wasm-bindgen's Node loader compiles file-backed bytes first, then
// synchronously constructs an Instance with its glue imports object.
const fileModule = new WebAssembly.Module(readFileSync(addWasmPath));
const fileInstance = new WebAssembly.Instance(fileModule);
console.log("fileInstance=" + fileInstance.exports.add(9, 12));

const importedModule = new WebAssembly.Module(readFileSync(importedWasmPath));
const importedInstance = new WebAssembly.Instance(importedModule, {
    "./glue": { inc: (value: number) => value + 1 },
});
console.log("fileImported=" + importedInstance.exports.call(20));
console.log("instanceLength=" + WebAssembly.Instance.length);
"#;

const GLUE_FIXTURE: &str = r#"
import { call } from "./imported.wasm";

export function inc(value: number): number {
    return value + 1;
}

export function throughGlue(value: number): number {
    return call(value);
}
"#;

const TABLE_GLUE_FIXTURE: &str = r#"
import { refs, start } from "./externref-table-cycle.wasm";

export function init(): void {
    refs.grow(2);
    refs.set(3, true);
}

export function runTableCycle(): string {
    start();
    return refs.length + ":" + refs.get(3);
}
"#;

fn write_fixture(root: &std::path::Path) {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(ADD_WASM_BASE64)
        .expect("decode add.wasm");
    assert_eq!(bytes.len(), 41);
    std::fs::write(root.join("add.wasm"), &bytes).expect("write add.wasm");
    std::fs::write(root.join("imported.wasm"), IMPORTED_WASM).expect("write imported.wasm");
    std::fs::write(root.join("file-add.wasm"), bytes).expect("write file add.wasm");
    std::fs::write(root.join("file-imported.wasm"), IMPORTED_WASM)
        .expect("write file imported.wasm");
    std::fs::write(root.join("memory.wasm"), MEMORY_WASM).expect("write memory.wasm");
    std::fs::write(root.join("live-memory.wasm"), LIVE_MEMORY_MULTI_WASM)
        .expect("write live memory wasm");
    std::fs::write(root.join("externref-table.wasm"), EXTERNREF_TABLE_WASM)
        .expect("write externref table wasm");
    std::fs::write(
        root.join("externref-table-cycle.wasm"),
        EXTERNREF_TABLE_CYCLE_WASM,
    )
    .expect("write externref table cycle wasm");
    std::fs::write(root.join("glue.ts"), GLUE_FIXTURE).expect("write glue.ts");
    std::fs::write(root.join("table-glue.ts"), TABLE_GLUE_FIXTURE).expect("write table glue ts");
    std::fs::write(root.join("main.ts"), MAIN_FIXTURE).expect("write main.ts");
}

#[test]
fn wasm_esm_import_instantiates_and_exposes_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fixture(dir.path());
    let output_path = dir.path().join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(dir.path().join("main.ts"))
        .arg("-o")
        .arg(&output_path)
        .arg("--no-cache")
        .arg("--strict-dynamic-import")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RS4GC", "0")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
        .output()
        .expect("compile wasm ESM fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    assert!(
        !stderr.contains("ahead-of-time-unsupported site")
            && !stderr.contains("full .wasm ESM instantiation is tracked"),
        "a real wasm import must not be reported as deferred:\n{stderr}"
    );

    let run = Command::new(&output_path)
        .output()
        .expect("run wasm ESM fixture");
    assert!(
        run.status.success(),
        "binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("namespace=5"), "stdout:\n{stdout}");
    assert!(stdout.contains("named=15"), "stdout:\n{stdout}");
    assert!(stdout.contains("default=42"), "stdout:\n{stdout}");
    assert!(stdout.contains("imported=42"), "stdout:\n{stdout}");
    assert!(stdout.contains("circular=9"), "stdout:\n{stdout}");
    assert!(stdout.contains("memory=65536"), "stdout:\n{stdout}");
    assert!(stdout.contains("defaultMemory=65536"), "stdout:\n{stdout}");
    assert!(stdout.contains("liveLoad=42"), "stdout:\n{stdout}");
    assert!(stdout.contains("liveStore=99"), "stdout:\n{stdout}");
    assert!(stdout.contains("multi=7:3"), "stdout:\n{stdout}");
    assert!(stdout.contains("table=2:5:true"), "stdout:\n{stdout}");
    assert!(stdout.contains("tableCycle=4:true"), "stdout:\n{stdout}");
    assert!(stdout.contains("fileInstance=21"), "stdout:\n{stdout}");
    assert!(stdout.contains("fileImported=21"), "stdout:\n{stdout}");
    assert!(stdout.contains("instanceLength=1"), "stdout:\n{stdout}");
}
