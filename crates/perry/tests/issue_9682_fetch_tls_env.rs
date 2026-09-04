//! End-to-end regression coverage for #9682.
//!
//! Global `fetch()` and `node:https` must consume one process-wide TLS trust
//! policy. Each case runs in a fresh process because Node scopes CA-file
//! discovery to process startup and Perry caches its pooled HTTP clients.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TLS_ENV_VARS: [&str; 3] = [
    "NODE_TLS_REJECT_UNAUTHORIZED",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
];

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

struct TlsServer {
    child: Child,
    port: u16,
}

impl Drop for TlsServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_tls_server(workspace: &Path) -> TlsServer {
    let key = workspace.join("test-parity/node-suite/tls/fixtures/localhost-key.pem");
    let certificate = workspace.join("crates/perry/tests/fixtures/issue_9682-leaf-cert.pem");
    let script = r#"
const fs = require("node:fs");
const https = require("node:https");
const server = https.createServer(
  { key: fs.readFileSync(process.argv[1]), cert: fs.readFileSync(process.argv[2]) },
  (_request, response) => response.end("ok"),
);
server.listen(0, "127.0.0.1", () => {
  process.stdout.write(String(server.address().port) + "\n");
});
"#;
    let mut child = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(key)
        .arg(certificate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start local TLS fixture server with Node");
    let mut port_line = String::new();
    BufReader::new(child.stdout.take().expect("TLS server stdout"))
        .read_line(&mut port_line)
        .expect("read TLS server port");
    let port = port_line.trim().parse::<u16>().unwrap_or_else(|error| {
        panic!("TLS server did not report a port ({port_line:?}): {error}")
    });
    TlsServer { child, port }
}

fn compile_client(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(
        &entry,
        r#"
import { request } from "node:https";

function requestWithHttps(url: string): Promise<string> {
  return new Promise((resolve) => {
    const req = request(url, (response) => {
      response.on("data", () => {});
      response.on("end", () => resolve("https:ok"));
    });
    req.on("error", () => resolve("https:error"));
    req.end();
  });
}

async function main() {
  const url = process.env.PERRY_TLS_TEST_URL!;
  const fetchResult = await fetch(url).then(
    async (response) => {
      await response.text();
      return "fetch:ok";
    },
    () => "fetch:error",
  );
  const httpsResult = await requestWithHttps(url);
  console.log(fetchResult + " " + httpsResult);
}

main();
"#,
    )
    .expect("write TLS client fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("--no-cache")
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run_with_timeout(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run compiled TLS fixture");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait().expect("poll compiled TLS fixture") {
            Some(_) => {
                return child
                    .wait_with_output()
                    .expect("collect TLS fixture output")
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .expect("collect timed-out TLS output");
                panic!(
                    "compiled TLS fixture timed out\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn run_case(binary: &Path, url: &str, environment: Option<(&str, &std::ffi::OsStr)>) -> String {
    let mut command = Command::new(binary);
    command.env("PERRY_TLS_TEST_URL", url);
    for variable in TLS_ENV_VARS {
        command.env_remove(variable);
    }
    if let Some((variable, value)) = environment {
        command.env(variable, value);
    }
    let output = run_with_timeout(&mut command);
    assert!(
        output.status.success(),
        "TLS case {environment:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn fetch_and_https_share_node_tls_environment() {
    let workspace = workspace_root();
    let ca = workspace.join("test-parity/node-suite/tls/fixtures/localhost-cert.pem");
    let server = start_tls_server(&workspace);
    let url = format!("https://127.0.0.1:{}/", server.port);
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile_client(dir.path());

    assert_eq!(run_case(&binary, &url, None), "fetch:error https:error\n");
    assert_eq!(
        run_case(
            &binary,
            &url,
            Some(("NODE_TLS_REJECT_UNAUTHORIZED", std::ffi::OsStr::new("0"))),
        ),
        "fetch:ok https:ok\n"
    );
    for variable in ["NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE"] {
        assert_eq!(
            run_case(&binary, &url, Some((variable, ca.as_os_str()))),
            "fetch:ok https:ok\n",
            "{variable} must configure both HTTP surfaces"
        );
    }
}
