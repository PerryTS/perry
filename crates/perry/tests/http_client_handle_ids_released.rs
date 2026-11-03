//! A finished http client exchange gives its two shared-band handle ids (the
//! ClientRequest and its IncomingMessage) back once a full trace proves that
//! nothing names them (perry-feather#111). 140k sequential-in-batches requests
//! need 280k ids, more than the band holds, so without that release the
//! program fails with ERR_PERRY_HANDLE_IDS_EXHAUSTED. Kept pairs must keep
//! answering like Node and stay distinct from every later pair. Expected
//! output is node's.

use std::process::Command;

#[test]
fn client_requests_give_their_handle_ids_back() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    std::fs::write(
        &entry,
        include_str!("fixtures/http_client_handle_id_pressure.ts"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "run failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "requests ok 140000 of 140000 errors []\n\
         kept 140 same as a later one 0 weak hits 0\n\
         kept pairs answering like node 140\n"
    );
}
