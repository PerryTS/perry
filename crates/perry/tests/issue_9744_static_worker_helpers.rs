//! Worker helper resolution must discover and start the real worker entry.
use std::path::Path;
use std::process::Command;

fn compile_and_run(dir: &Path, source: &str, bun: bool) -> String {
    let entry = dir.join("main.ts");
    let binary = dir.join("app");
    std::fs::write(&entry, source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_perry"));
    command
        .current_dir(dir)
        .args(["compile"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1");
    if bun {
        command.args(["--platform", "bun", "--bunfs-root"]).arg(dir);
    }
    let compile = command.output().unwrap();
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(compile.status.success(), "{diagnostics}");
    assert!(
        !diagnostics.contains("this Worker will throw"),
        "{diagnostics}"
    );
    let run = Command::new(binary).output().unwrap();
    assert!(
        run.status.success(),
        "status={}\nstdout={}\nstderr={}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).unwrap()
}

#[test]
fn bun_global_worker_starts_from_an_embedded_file_url_helper_chain() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("worker.js"), "postMessage('ready');").unwrap();
    let stdout = compile_and_run(
        dir.path(),
        r#"
        const embeddedWorkerUrl = (path) => new URL(`file://${path}`);
        const hooksWorkerUrl = () => embeddedWorkerUrl('/$bunfs/root/worker.js');
        const worker = new Worker(hooksWorkerUrl());
        worker.onmessage = ({ data }) => {
            console.log('global', data);
            worker.terminate().then(() => process.exit(0));
        };
        setTimeout(() => process.exit(2), 5000);
    "#,
        true,
    );
    assert_eq!(stdout.trim(), "global ready");
}

#[test]
fn node_worker_starts_through_declarations_aliases_and_url_arguments() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("_helpers")).unwrap();
    std::fs::write(
        dir.path().join("_helpers/static_worker_9744.ts"),
        include_str!("../../../test-files/_helpers/static_worker_9744.ts"),
    )
    .unwrap();
    let stdout = compile_and_run(
        dir.path(),
        include_str!("../../../test-files/test_gap_9744_static_worker_helpers.ts"),
        false,
    );
    assert_eq!(stdout.trim(), "node ready");
}

#[test]
fn helper_file_urls_decode_filesystem_paths() {
    let dir = tempfile::tempdir().unwrap();
    let worker = dir.path().join("worker space.ts");
    std::fs::write(
        &worker,
        include_str!("../../../test-files/_helpers/static_worker_9744.ts"),
    )
    .unwrap();
    let url = url::Url::from_file_path(worker).unwrap();
    assert!(url.as_str().contains("%20"));
    let url_literal = serde_json::to_string(url.as_str()).unwrap();
    let source = format!(
        r#"
        import {{ Worker }} from 'node:worker_threads';
        const entry = () => new URL({url_literal});
        const worker = new Worker(entry());
        worker.on('message', (data) => {{
            console.log('file', data);
            worker.terminate().then(() => process.exit(0));
        }});
        setTimeout(() => process.exit(2), 5000);
    "#
    );
    assert_eq!(
        compile_and_run(dir.path(), &source, false).trim(),
        "file ready"
    );
}
