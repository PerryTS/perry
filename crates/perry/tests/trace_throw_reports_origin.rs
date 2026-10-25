//! `PERRY_TRACE_THROW=1` reports every thrown value at `js_throw` time, so an
//! error that is caught and swallowed (or rethrown) is still visible at its
//! origin. With the knob off nothing is printed.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn a_caught_throw_is_reported_only_when_the_knob_is_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.js");
    std::fs::write(
        &entry,
        "function inner() { throw new Error('swallowed-boom'); }\n\
         try { inner(); } catch (e) { console.log('caught', e.message); }\n",
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let quiet = Command::new(&output)
        .env_remove("PERRY_TRACE_THROW")
        .output()
        .expect("run compiled binary");
    assert!(quiet.status.success());
    assert_eq!(
        String::from_utf8_lossy(&quiet.stdout),
        "caught swallowed-boom\n"
    );
    assert!(!String::from_utf8_lossy(&quiet.stderr).contains("[trace-throw]"));

    let traced = Command::new(&output)
        .env("PERRY_TRACE_THROW", "1")
        .output()
        .expect("run compiled binary");
    assert!(traced.status.success());
    assert_eq!(
        String::from_utf8_lossy(&traced.stdout),
        "caught swallowed-boom\n"
    );
    let stderr = String::from_utf8_lossy(&traced.stderr);
    assert!(
        stderr.contains("[trace-throw] thrown value:"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("swallowed-boom"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("[trace-throw] throw site"),
        "stderr:\n{stderr}"
    );
}
