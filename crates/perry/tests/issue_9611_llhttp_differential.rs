//! #9611: the llhttp differential — the verification the issue asked for.
//!
//! cc's only real WebAssembly is undici's llhttp, on the network path, and the
//! issue's acceptance criterion for making `WebAssembly.Memory.prototype.buffer`
//! alias the engine's linear memory was that the *same HTTP bytes* through the
//! *same wasm* produce byte-identical parse results under perry and node.
//!
//! `fixtures/llhttp/driver.ts` drives the real llhttp builds the way undici
//! drives them — a windowed `Uint8Array` over linear memory is filled with the
//! socket chunk, `llhttp_execute` runs, the parser calls back into JS — across
//! whole-message, byte-at-a-time, and 4 KiB chunkings of simple, chunked,
//! pipelined, 100-continue, many-header and 300 KiB-body responses. Every
//! callback span is read BOTH out of linear memory and through undici's own
//! trick of mapping the wasm pointer back into the source chunk, so a span that
//! is right in one view and wrong in the other fails loudly rather than
//! silently agreeing with itself.
//!
//! `fixtures/llhttp/expected.txt` is node's trace, byte for byte. Both llhttp
//! builds produce it identically.
//!
//! This test is also the regression pin for the bug the differential FOUND: the
//! wasm host used to hold the imports object as raw NaN-boxed bits, so a
//! collection triggered inside one import callback left every later import in
//! the same call reading a relocated object. `call_wasm_import` then returned 0
//! and the host substituted the import's default result, so wasm ran on with no
//! error — through llhttp that silently dropped `on_message_complete`, i.e. a
//! truncated HTTP response reported as a clean parse. The 300 KiB-body case
//! allocates enough inside `wasm_on_body` to trigger that collection.

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

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/llhttp")
}

/// Dedicated target dir for this suite's `wasm-host`-enabled runtime, for the
/// same reason `issue_5234_wasm_esm_import` has one (#8547): built into the
/// shared `target/debug` it would replace `libperry_runtime.a` with one
/// carrying undefined `perry_wasm_host_*` references, breaking every other
/// suite that links runtime-only.
fn wasm_host_target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("perry-wasm-host-test")
}

fn ensure_runtime_archives() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(&cargo)
            .current_dir(workspace_root())
            .env("CARGO_TARGET_DIR", wasm_host_target_dir())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("--features")
            .arg("perry-runtime/wasm-host")
            .output()
            .expect("build static runtime wrapper with wasm host shims");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archives();
    wasm_host_target_dir().join("debug")
}

/// Report the FIRST differing line rather than dumping two 1,200-line traces:
/// the position is the diagnosis (which case, which callback).
fn assert_trace_matches(expected: &str, actual: &str, label: &str) {
    if expected == actual {
        return;
    }
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    let mut line_no = 0usize;
    loop {
        line_no += 1;
        match (expected_lines.next(), actual_lines.next()) {
            (None, None) => break,
            (a, b) if a == b => continue,
            (a, b) => panic!(
                "{label}: trace diverges from node at line {line_no}\n  \
                 node : {}\n  perry: {}\n\n\
                 (expected {} lines, got {})",
                a.unwrap_or("<end of trace>"),
                b.unwrap_or("<end of trace>"),
                expected.lines().count(),
                actual.lines().count(),
            ),
        }
    }
    panic!("{label}: traces differ only in trailing newline");
}

#[test]
fn llhttp_parses_identically_to_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let driver = fixtures().join("driver.ts");
    let expected = std::fs::read_to_string(fixtures().join("expected.txt")).expect("read oracle");
    let binary = dir.path().join("llhttp_driver");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&driver)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RS4GC", "0")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
        .output()
        .expect("compile llhttp driver");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // Both builds undici ships, because they are both what really loads.
    for wasm in ["llhttp.wasm", "llhttp_simd.wasm"] {
        let run = Command::new(&binary)
            .arg(fixtures().join(wasm))
            .output()
            .unwrap_or_else(|e| panic!("run llhttp driver on {wasm}: {e}"));
        assert!(
            run.status.success(),
            "{wasm}: driver failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let actual = String::from_utf8_lossy(&run.stdout);
        assert_trace_matches(expected.trim_end(), actual.trim_end(), wasm);
    }
}
