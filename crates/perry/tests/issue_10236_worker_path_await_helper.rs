//! OpenCode's awaited TUI worker selector discovers a union of native entries.
use std::path::Path;
use std::process::{Command, Output};

fn diagnostics(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn compile(root: &Path, source: &str) -> String {
    std::fs::write(root.join("main.ts"), source).unwrap();
    let worker = root.join("worker.ts");
    let define = format!(
        "WORKER_PATH={}",
        serde_json::to_string(&worker.to_string_lossy()).unwrap()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(root)
        .args([
            "compile",
            "main.ts",
            "-o",
            "app",
            "--platform",
            "bun",
            "--define",
        ])
        .arg(define)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_NO_CACHE", "1")
        .output()
        .unwrap();
    let log = diagnostics(&output);
    assert!(output.status.success(), "{log}");
    log
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("worker.ts"), "postMessage('ready');").unwrap();
    dir
}

fn run(root: &Path, args: &[&str]) -> String {
    let output = Command::new(root.join("app"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        output.status,
        diagnostics(&output)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

const START: &str = r#"
const worker = new Worker(file, { env: { WORKER_TEST: '10236' } });
worker.onmessage = ({ data }) => {
    console.log('reply', data);
    worker.terminate().then(() => process.exit(0));
};
setTimeout(() => process.exit(2), 5000);
"#;

#[test]
fn opencode_awaited_if_return_helper_skips_missing_and_deduplicates_entry() {
    let dir = fixture();
    let source = format!(
        r#"
import {{ existsSync as exists }} from 'node:fs';
import {{ fileURLToPath }} from 'node:url';
declare const WORKER_PATH: string;
async function target() {{
    if (typeof WORKER_PATH !== 'undefined') return WORKER_PATH;
    const dist = new URL('../x/worker.js', import.meta.url);
    if (await exists(fileURLToPath(dist))) return dist;
    return new URL('./worker.ts', import.meta.url);
}}
const file = await target();
{START}
"#
    );
    let log = compile(dir.path(), &source);
    assert!(!log.contains("Worker path helper:"), "{log}");
    assert!(!log.contains("this Worker will throw"), "{log}");
    assert!(
        log.contains("skipping candidate") && log.contains("../x/worker.js"),
        "{log}"
    );
    // Discovery can run twice when platform dependencies trigger recollection.
    // The graph summary must still contain only main plus one worker.
    assert!(log.contains("Found 2 module(s): 2 native"), "{log}");
    assert_eq!(run(dir.path(), &[]), "reply ready\n");
}

#[test]
fn sync_two_returns_dispatches_both_existing_entries_and_preserves_effects() {
    let dir = fixture();
    std::fs::write(dir.path().join("other space.ts"), "postMessage('other');").unwrap();
    let source = format!(
        r#"
function choose() {{ console.log('choose'); return process.argv.includes('--other'); }}
function target() {{
    if (choose()) return new URL('./other space.ts', import.meta.url);
    return './worker.ts';
}}
const file = target();
{START}
"#
    );
    let log = compile(dir.path(), &source);
    assert!(!log.contains("this Worker will throw"), "{log}");
    assert!(log.contains("Found 3 module(s): 3 native"), "{log}");
    assert_eq!(run(dir.path(), &[]), "choose\nreply ready\n");
    assert_eq!(run(dir.path(), &["--other"]), "choose\nreply other\n");
}

#[test]
fn await_literal_returning_async_helper() {
    let dir = fixture();
    let source = format!(
        "async function target() {{ return './worker.ts'; }} const file = await target(); {START}"
    );
    let log = compile(dir.path(), &source);
    assert!(!log.contains("Worker path helper:"), "{log}");
    assert_eq!(run(dir.path(), &[]), "reply ready\n");
}

#[test]
fn missing_runtime_selection_throws_instead_of_starting_another_candidate() {
    let dir = fixture();
    let source = r#"
function target() {
    if (process.argv.includes('--missing')) return './missing.ts';
    return './worker.ts';
}
try {
    new Worker(target());
    console.log('unexpected worker');
    process.exit(2);
} catch (error) { console.log('caught', error.message); }
"#;
    let log = compile(dir.path(), source);
    assert!(log.contains("skipping candidate"), "{log}");
    assert!(run(dir.path(), &["--missing"])
        .contains("did not match an existing compile-time-resolved worker entry"));
}

#[test]
fn opaque_return_and_recursive_helper_keep_existing_warnings() {
    for (helper, warning) in [
        ("async function target() { if (process.argv.length) return './worker.ts'; return opaque(); }", "opaque call target"),
        ("async function target() { if (process.argv.length) return './worker.ts'; return await target(); }", "recursive helper call"),
    ] {
        let dir = fixture();
        let source = format!("const opaque: any = process.argv[0]; {helper} async function cold() {{ const file = await target(); new Worker(file); }} console.log('cold');");
        let log = compile(dir.path(), &source);
        assert!(log.contains("Worker path helper:") && log.contains(warning), "{log}");
        assert!(log.contains("this Worker will throw"), "{log}");
        assert_eq!(run(dir.path(), &[]), "cold\n");
    }
}
