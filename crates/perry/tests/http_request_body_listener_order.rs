//! A server request body reaches every listener order Node supports:
//! `'end'` registered before `'data'` (effect's NodeStream.toArrayBuffer does
//! `once("error")`, `once("end")`, then `on("data")`), `once` for `'end'` and
//! `'data'`, several `'end'` listeners, and listeners attached on a later
//! turn. `req.once` was missing from the request's dynamic surface (a silent
//! no-op), and an `'end'` registered before `'data'` fired at once and lost
//! the body; opencode's server never answered `POST /api/session`.
//! Expected output is node's.

use std::process::Command;

#[test]
fn request_body_reaches_every_listener_order() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join(if cfg!(windows) { "main.exe" } else { "main" });
    std::fs::write(&entry, SOURCE).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(output.status.success(), "compile failed: {}", String::from_utf8_lossy(&output.stderr));
    let output = Command::new(&binary).current_dir(dir.path()).output().unwrap();
    assert!(
        output.status.success(),
        "run failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), EXPECTED);
}

const EXPECTED: &str = "/data-then-end function body:14\n/end-then-data function body:14\n/once-end-then-data function body:19\n/data-then-once-end function body:19\n/once-data function body:10\n/two-ends function body:9 x2\n/later function body:6\n/empty-end function empty\n";

const SOURCE: &str = r#"import { createServer } from "node:http";
const routes: Record<string, (req: any, done: (s: string) => void) => void> = {
  "/data-then-end": (req, done) => { const b: Buffer[] = []; req.on("data", (c: Buffer) => b.push(c)); req.on("end", () => done(Buffer.concat(b).toString())); },
  "/end-then-data": (req, done) => { const b: Buffer[] = []; req.on("end", () => done(Buffer.concat(b).toString())); req.on("data", (c: Buffer) => b.push(c)); },
  "/once-end-then-data": (req, done) => { const b: Buffer[] = []; req.once("error", () => done("error")); req.once("end", () => done(Buffer.concat(b).toString())); req.on("data", (c: Buffer) => b.push(c)); },
  "/data-then-once-end": (req, done) => { const b: Buffer[] = []; req.on("data", (c: Buffer) => b.push(c)); req.once("end", () => done(Buffer.concat(b).toString())); },
  "/once-data": (req, done) => { const b: Buffer[] = []; req.once("data", (c: Buffer) => b.push(c)); req.on("end", () => done(Buffer.concat(b).toString())); },
  "/two-ends": (req, done) => { const b: Buffer[] = []; let n = 0; const fin = () => { if (++n === 2) done(Buffer.concat(b).toString() + " x2"); }; req.once("end", fin); req.on("data", (c: Buffer) => b.push(c)); req.on("end", fin); },
  "/later": (req, done) => { setTimeout(() => { const b: Buffer[] = []; req.once("end", () => done(Buffer.concat(b).toString())); req.on("data", (c: Buffer) => b.push(c)); }, 20); },
  "/empty-end": (req, done) => { req.once("end", () => done("empty")); req.on("data", () => {}); },
};
const server = createServer((req, res) => {
  const route = routes[req.url!];
  route(req, (s) => res.end(`${req.url} ${typeof req.once} ${s}`));
});
server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  for (const path of Object.keys(routes)) {
    const body = path === "/empty-end" ? "" : "body:" + path.length;
    const text = await fetch(`http://127.0.0.1:${port}${path}`, { method: "POST", body, signal: AbortSignal.timeout(3000) })
      .then((r) => r.text()).catch((e) => `${path} ${e.name}`);
    console.log(text);
  }
  server.close();
});
"#;
