//! Compile/run #10324 against both nodemailer providers. Port 1 on loopback
//! gives a bounded refusal, proving `verify` and `sendMail` reached the
//! registered transporter without sending any external mail.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Once;
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn target_debug_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
        .parent()
        .expect("perry binary directory")
        .to_path_buf()
}

fn ensure_runtime_archives() {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command.current_dir(workspace_root()).args([
            "build",
            "-p",
            "perry",
            "-p",
            "perry-runtime-static",
            "-p",
            "perry-stdlib-static",
            "-p",
            "perry-ext-nodemailer",
            "-p",
            "perry-ext-net",
        ]);
        let binary_path = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
        let binary_profile_dir = binary_path
            .parent()
            .expect("Cargo binary profile directory");
        let profile = binary_profile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("Cargo profile name");
        if profile == "release" {
            command.arg("--release");
        } else if profile != "debug" {
            command.args(["--profile", profile]);
        }
        if let Some(target) = std::env::var_os("CARGO_BUILD_TARGET") {
            command.arg("--target").arg(target);
        } else {
            let configured_target_dir = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .map(|path| {
                    if path.is_absolute() {
                        path
                    } else {
                        workspace_root().join(path)
                    }
                })
                .unwrap_or_else(|| workspace_root().join("target"));
            let actual_target_dir = binary_profile_dir.parent().expect("Cargo target directory");
            if let Ok(relative_target) = actual_target_dir.strip_prefix(configured_target_dir) {
                if relative_target.components().count() == 1 {
                    command.arg("--target").arg(relative_target);
                }
            }
        }
        let build = command
            .output()
            .expect("build the matching runtime/provider archives");
        assert!(
            build.status.success(),
            "archive build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

const SOURCE: &str = r#"
import * as nodemailer from "nodemailer";
const config = { host: "127.0.0.1", port: 1, secure: false };
const transport = nodemailer.createTransport(config);
const second = nodemailer.createTransport(config);
const dynamic: any = transport;
const holder: any = { transport };
const message = {
  from: "sender@example.test",
  to: "recipient@example.test",
  subject: "Perry local SMTP dispatch test",
  text: "This test sends no external mail.",
};
async function verify(label: string, promise: Promise<any>): Promise<void> {
  try {
    await promise;
    console.log(`${label}:verify-resolved`);
  } catch (error) {
    console.log(`${label}:verify-rejected:${String(error)}`);
  }
}
async function send(label: string, promise: Promise<any>): Promise<void> {
  try {
    await promise;
    console.log(`${label}:send-resolved`);
  } catch (error) {
    console.log(`${label}:send-rejected:${String(error)}`);
  }
}
async function main(): Promise<void> {
  console.log(`typeof:${typeof transport}`);
  console.log(`identity:${transport === second ? "same" : "distinct"}`);
  await verify("direct", transport.verify());
  await verify("dynamic", dynamic.verify());
  await verify("property", holder.transport.verify());
  await Promise.resolve().then(() => verify("then", dynamic.verify()));
  await send("direct", transport.sendMail(message));
  await send("dynamic", dynamic.sendMail(message));
  await send("property", holder.transport.sendMail(message));
  await Promise.resolve().then(() => send("then", dynamic.sendMail(message)));
}
main();
"#;

fn compile_and_run(dir: &Path, disabled_well_known: bool) -> Output {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write fixture");
    let runtime_dir = target_debug_dir();
    let mut compile = Command::new(perry_bin());
    compile
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
        .env("PERRY_RUNTIME_DIR", &runtime_dir)
        .env("PERRY_LIB_DIR", &runtime_dir);
    if disabled_well_known {
        compile.env("PERRY_DISABLE_WELL_KNOWN", "1");
    } else {
        compile.env_remove("PERRY_DISABLE_WELL_KNOWN");
    }
    let result = compile.output().expect("run perry compile");
    assert!(
        result.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let mut child = Command::new(output)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run compiled fixture");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().expect("poll compiled fixture").is_some() {
            return child
                .wait_with_output()
                .expect("collect compiled fixture output");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("compiled fixture timed out during local SMTP refusal");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn bundled_and_external_transporters_keep_object_handles_and_dispatch_methods() {
    ensure_runtime_archives();

    for disabled_well_known in [false, true] {
        let dir = tempfile::tempdir().expect("tempdir");
        let run = compile_and_run(dir.path(), disabled_well_known);
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(
            run.status.success(),
            "compiled fixture failed\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert!(
            stdout.contains("typeof:object\nidentity:distinct\n"),
            "{stdout}"
        );
        for method in ["verify", "send"] {
            for receiver in ["direct", "dynamic", "property", "then"] {
                let prefix = format!("{receiver}:{method}-rejected:");
                let line = stdout
                    .lines()
                    .find(|line| line.starts_with(&prefix))
                    .unwrap_or_else(|| panic!("missing {prefix} in:\n{stdout}"));
                assert!(
                    line.contains("ECONNREFUSED"),
                    "{line} should be the expected local SMTP refusal"
                );
            }
        }
    }
}
