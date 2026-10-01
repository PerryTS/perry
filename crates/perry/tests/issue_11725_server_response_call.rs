//! #11725: native ServerResponse construction must preserve explicit `this`.
use std::process::Command;

const RESPONSE: &str = r#"
const http = require('node:http');
const { Writable } = require('node:stream');
const util = require('node:util');

function Response(req) {
  http.ServerResponse.call(this, req);
  const socket = new Writable({ write(c, e, cb) { cb(); } });
  this.assignSocket(socket);
  console.log(typeof this.socket, typeof this.connection);
  console.log(this.socket === socket, this.connection === socket);
  let called = false;
  this.connection.once('probe', () => { called = true; });
  socket.emit('probe');
  console.log('once', called);
  this.setHeader('x-probe', 'ok');
  console.log('header', this.getHeader('x-probe'));
}
util.inherits(Response, http.ServerResponse);
module.exports = Response;
"#;

const INVOCATIONS: &[&str] = &[
    "http.ServerResponse.call(this, req);",
    "http.ServerResponse.apply(this, [req]);",
    "const args = [req]; http.ServerResponse.apply(this, args);",
];

fn check(cjs: bool, invocation: &str) {
    let response = RESPONSE.replace("http.ServerResponse.call(this, req);", invocation);
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    let main = if cjs {
        std::fs::write(dir.path().join("response.cjs"), &response).unwrap();
        "import Response from './response.cjs'; new (Response as any)({ method: 'GET' });"
            .to_string()
    } else {
        format!(
            "{}\nnew (Response as any)({{ method: 'GET' }});",
            response
                .replace("module.exports = Response;", "")
                .replace(
                    "const http = require('node:http');",
                    "import * as http from 'node:http';"
                )
                .replace(
                    "const { Writable } = require('node:stream');",
                    "import { Writable } from 'node:stream';"
                )
                .replace(
                    "const util = require('node:util');",
                    "import * as util from 'node:util';"
                )
        )
    };
    std::fs::write(&entry, main).unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--no-cache", "--no-auto-optimize"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(binary)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "run: {}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8(run.stdout).unwrap().replace("\r\n", "\n"),
        "object object\ntrue true\nonce true\nheader ok\n"
    );
}

#[test]
fn commonjs_inherits_response_assigns_socket() {
    for invocation in INVOCATIONS {
        check(true, invocation);
    }
}

#[test]
fn typescript_inherits_response_assigns_socket() {
    for invocation in INVOCATIONS {
        check(false, invocation);
    }
}
