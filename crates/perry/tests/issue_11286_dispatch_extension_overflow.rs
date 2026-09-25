//! Regression test for #11286: the runtime's handle dispatch-extension table
//! held four entries while five crates register a method extension
//! (perry-ext-http's server and client halves, perry-ext-net, perry-ext-ws,
//! perry-ext-nodemailer). Registration is lazy — each crate registers on its
//! first use — and the overflow path overwrote the LAST slot, so the fourth
//! crate to initialize silently lost its dynamic method dispatch.
//!
//! Here ws initializes fourth and nodemailer fifth. On the unfixed runtime an
//! untyped `wss.address()` returned `undefined` and `wss.close()` was a silent
//! no-op, which left the listening server alive and the process hung.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn compile_and_run(dir: &Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    // The unfixed runtime never closes the server, so bound the run instead
    // of letting the test hang.
    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run compiled binary");
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll compiled binary") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .expect("stdout pipe")
        .read_to_string(&mut stdout)
        .expect("read stdout");
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    let status = status.unwrap_or_else(|| {
        panic!(
            "compiled binary did not exit within 30 s (server never closed)\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        )
    });
    assert!(
        status.success(),
        "compiled binary failed\nstatus: {status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn fifth_dispatch_extension_does_not_evict_the_fourth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import http from "node:http";
import net from "node:net";
import { WebSocketServer } from "ws";
import nodemailer from "nodemailer";

// First-use order decides registration order: http server, http client, net,
// ws (fourth), nodemailer (fifth).
const server = http.createServer((_req, res) => { res.end("ok"); });
const agent = new http.Agent();
const tcp = net.createServer();
const wss = new WebSocketServer({ port: 0 });
const transporter = nodemailer.createTransport({ host: "127.0.0.1", port: 1 });

// An untyped receiver routes through the runtime's handle dispatch.
const w: any = [wss][0];
const addr = w.address();
console.log("ws address:", typeof addr, typeof addr?.port);
w.close();
console.log("ws closed");
server.close();
tcp.close();
agent.destroy();
"#,
    );

    assert_eq!(stdout, "ws address: object number\nws closed\n");
}
