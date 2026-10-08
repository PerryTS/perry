//! A native-this alias (`http.ServerResponse.call(this, req)`, #10454; the
//! light-my-request `Response` that fastify's `inject` builds per request)
//! must not keep its object alive. The alias used to sit in a thread-local
//! table that rooted every aliased object forever: a churn of responses kept
//! every one of them, and the table's linear lookup cost grew with it.
use std::process::Command;

const CHURN: &str = r#"
import * as http from "node:http";
import * as util from "node:util";
import { Writable } from "node:stream";
declare function gc(): void;
const H: any = http;
function Response(this: any, req: any) {
  H.ServerResponse.call(this, req);
  this.payload = new Array(256).fill(0);
  this.once('close', () => { this.payload[0] = 1; });
  this.once('error', () => { this.payload[0] = 2; });
  const socket = new Writable({ write(chunk: any, encoding: any, cb: any) { cb(); } });
  socket.on('error', () => { this.payload[0] = 3; });
  this.assignSocket(socket);
}
util.inherits(Response as any, H.ServerResponse);
const weak: WeakRef<any>[] = [];
let kept: any = null;
for (let i = 0; i < 20000; i++) {
  const r = new (Response as any)({ method: "GET" });
  r.setHeader("x-i", String(i));
  if (i % 200 === 0) weak.push(new WeakRef(r));
  if (i === 1234) kept = r;
}
setTimeout(() => {
  gc();
  setTimeout(() => {
    gc();
    const live = weak.filter((w) => w.deref() !== undefined).length;
    console.log("tracked " + weak.length);
    console.log("live " + live);
    console.log("kept " + kept.getHeader("x-i") + " " + (kept.setHeader("y", "1") === kept));
  }, 0);
}, 0);
"#;

#[test]
fn a_churn_of_aliased_responses_is_collected() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    std::fs::write(&entry, CHURN).unwrap();
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
    let stdout = String::from_utf8(run.stdout).unwrap().replace("\r\n", "\n");
    assert!(
        run.status.success(),
        "run: {stdout}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let field = |name: &str| -> usize {
        stdout
            .lines()
            .find_map(|l| l.strip_prefix(name)?.trim().parse().ok())
            .unwrap_or_else(|| panic!("no `{name}` line in:\n{stdout}"))
    };
    let (tracked, live) = (field("tracked "), field("live "));
    assert_eq!(tracked, 100, "premise: the churn tracked its responses");
    assert_eq!(
        live, 0,
        "every dropped response must be collected: {live}/{tracked} still live\n{stdout}"
    );
    assert!(
        stdout.contains("kept 1234 true"),
        "a referenced response keeps its alias:\n{stdout}"
    );
}
