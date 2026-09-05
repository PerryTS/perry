//! #8907: the v0.5.1220 macOS arm64 release could not link node:http.
//!
//! #5983 removed the external HTTP pump from the full stdlib. The #8587
//! feature-graph guard protects that fix, but does not exercise the linker.
//! Stage a source-free installation with the full archives and compile the
//! reported server in both default and PERRY_NO_AUTO_OPTIMIZE modes. This
//! must work without a source checkout repairing the libraries via auto-opt.

// The macOS/Linux CLI is self-contained. Windows additionally needs LLVM-C.dll
// staged beside the executable, which this Unix installation regression omits.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const HTTP_FIXTURE: &str = include_str!("../../../test-files/test_issue_8907_http_link.ts");
const ARCHIVES: [&str; 3] = [
    "libperry_runtime.a",
    "libperry_stdlib.a",
    "libperry_ext_http.a",
];

fn prebuilt_archives() -> Vec<PathBuf> {
    // A caller may supply a coherent, freshly built release bundle. Otherwise
    // build all three archives in one Cargo graph so stdlib and ext-http share
    // the same Tokio instance. Building only the missing wrapper can split it.
    if let Some(dir) = std::env::var_os("PERRY_RUNTIME_DIR") {
        let paths: Vec<_> = ARCHIVES
            .iter()
            .map(|name| PathBuf::from(&dir).join(name))
            .collect();
        if paths.iter().all(|path| path.is_file()) {
            return paths;
        }
    }

    let build = Command::new(env!("CARGO"))
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args([
            "build",
            "--release",
            "--message-format=json",
            "-p",
            "perry-runtime-static",
            "-p",
            "perry-stdlib-static",
            "-p",
            "perry-ext-http",
        ])
        .output()
        .expect("build coherent prebuilt HTTP archives");
    assert!(
        build.status.success(),
        "archive build failed:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let artifacts: Vec<serde_json::Value> = String::from_utf8_lossy(&build.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|item: &serde_json::Value| item["reason"] == "compiler-artifact")
        .collect();
    ARCHIVES
        .iter()
        .map(|filename| {
            artifacts
                .iter()
                .filter_map(|item| item["filenames"].as_array())
                .flatten()
                .filter_map(|value| value.as_str())
                .map(PathBuf::from)
                .find(|path| path.file_name().is_some_and(|file| file == *filename))
                .unwrap_or_else(|| panic!("Cargo did not produce {filename}"))
        })
        .collect()
}

fn run_with_timeout(binary: &Path, cwd: &Path) -> String {
    // File-backed output keeps a failing fixture from blocking on a full pipe.
    let stdout = tempfile::tempfile().expect("stdout file");
    let stderr = tempfile::tempfile().expect("stderr file");
    let mut child = Command::new(binary)
        .current_dir(cwd)
        .stdout(Stdio::from(stdout.try_clone().expect("clone stdout")))
        .stderr(Stdio::from(stderr.try_clone().expect("clone stderr")))
        .spawn()
        .expect("run compiled fixture");
    let deadline = Instant::now() + Duration::from_secs(30);
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().expect("poll fixture") {
            break (status, false);
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill hung fixture");
            break (child.wait().expect("reap hung fixture"), true);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    use std::io::{Read, Seek};
    let read = |mut file: std::fs::File| {
        file.rewind().expect("rewind output");
        let mut text = String::new();
        file.read_to_string(&mut text).expect("read output");
        text
    };
    let stdout = read(stdout);
    let stderr = read(stderr);
    assert!(
        !timed_out,
        "{} did not exit within 30 seconds:\n{stdout}\n{stderr}",
        binary.display()
    );
    assert!(
        status.success(),
        "fixture failed: {status}\n{stdout}\n{stderr}"
    );
    stdout
}

#[test]
fn source_free_http_server_links_and_closes_in_both_modes() {
    let archives = prebuilt_archives();
    let dir = tempfile::tempdir().expect("installation tempdir");
    let install = dir.path().join("install");
    let app = dir.path().join("app");
    std::fs::create_dir(&install).expect("create installation");
    std::fs::create_dir(&app).expect("create app directory");
    let compiler = install.join(format!("perry{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_perry"), &compiler).expect("stage compiler");
    for archive in archives {
        std::fs::copy(&archive, install.join(archive.file_name().unwrap())).expect("stage archive");
    }

    for no_auto in [false, true] {
        let mode = if no_auto { "no-auto" } else { "default" };
        for (name, source, expected) in [
            ("nohttp", "console.log('ok');", "ok\n"),
            ("httpmin", HTTP_FIXTURE, "listening\nclosed\n"),
        ] {
            let entry = app.join(format!("{name}-{mode}.ts"));
            let output = app.join(format!("{name}-{mode}{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&entry, source).expect("write fixture");
            let mut command = Command::new(&compiler);
            command
                .current_dir(&app)
                .env_remove("PERRY_WORKSPACE_ROOT")
                .env_remove("PERRY_DISABLE_WELL_KNOWN")
                .env_remove("PERRY_FORCE_WELL_KNOWN")
                .env_remove("PERRY_NO_AUTO_OPTIMIZE")
                .env("PERRY_RUNTIME_DIR", &install)
                .env("PERRY_LIB_DIR", &install)
                .arg("compile")
                .arg(&entry)
                .arg("-o")
                .arg(&output);
            if no_auto {
                command.env("PERRY_NO_AUTO_OPTIMIZE", "1");
            }
            let compile = command.output().expect("compile fixture");
            let stdout = String::from_utf8_lossy(&compile.stdout);
            let stderr = String::from_utf8_lossy(&compile.stderr);
            assert!(
                compile.status.success(),
                "{name} ({mode}) failed to link (#8907):\n{stdout}\n{stderr}"
            );
            if !no_auto {
                assert!(
                    stderr.contains("Perry workspace source not found"),
                    "{name}: default mode must exercise the source-free fallback:\n{stdout}\n{stderr}"
                );
            }
            assert_eq!(run_with_timeout(&output, &app), expected, "{name} ({mode})");
        }
    }
}
