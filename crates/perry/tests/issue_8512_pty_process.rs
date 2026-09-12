//! Compiled TypeScript reaches the shared native PTY through bun-pty, including
//! on headless Windows runners where the compiler's stdin is redirected.
#![cfg(any(unix, windows))]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn bun_pty_compiled_interactive_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let output = dir
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    std::fs::write(&entry, r#"
import { spawn } from "bun-pty";
const windows = process.platform === "win32";
const term = spawn(windows ? "cmd.exe" : "sh", windows ? ["/d", "/q"] : [], {
  name: "xterm-256color", cols: 80, rows: 24, cwd: process.cwd(), env: process.env,
});
let text = "";
term.onData((chunk: string) => { text += chunk; });
term.onExit((event: { exitCode: number }) => {
  if (!text.includes("native_ok")) throw new Error("PTY output missing: " + text);
  if (event.exitCode !== 0) throw new Error("unexpected exit code");
  console.log("PTY_OK");
});
term.resize(100, 40);
if (term.cols !== 100 || term.rows !== 40) throw new Error("resize failed");
term.pause();
term.write(windows ? "set part=ok\r\necho native_%part%\r\nexit\r\n" : "echo native_$(echo ok)\nexit\n");
setTimeout(() => term.resume(), 100);
"#).unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .args(["compile", "--no-auto-optimize"])
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(&output)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY fixture did not terminate");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let run = child.wait_with_output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("PTY_OK"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}
