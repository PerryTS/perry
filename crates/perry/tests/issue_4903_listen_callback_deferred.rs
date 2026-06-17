//! Regression test for #4903: the `listen(port, cb)` callback (and the
//! `'listening'` emit) must fire on a later event-loop tick, after the
//! current synchronous script segment finishes — never synchronously from
//! inside `listen()`. Pre-fix, the canonical Node corpus shape
//! `const server = http.createServer().listen(0, cb)` ran `cb` before
//! `server` was assigned, so `server.address().port` threw
//! "Cannot read properties of undefined (reading 'address')".
//!
//! Also covers the sibling `this`-binding half of the ticket: Node invokes
//! `'listening'` listeners, the listen callback, and `'request'` listeners
//! (including the `createServer(handler)` handler) with `this` bound to the
//! server, so `this.address().port` works inside both.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The chained corpus shape: the listen callback must see `server` already
/// assigned (deferred emit), `server.address()` must return a real
/// `{ address, family, port }`, and a full request round-trip must work with
/// `this` bound to the server inside the request handler.
#[test]
fn listen_callback_is_deferred_and_this_bound() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const http = require('http');
const server = http.createServer(function (req: any, res: any) {
  // #4903 — `'request'` listeners run with `this` = server.
  console.log('handler-this-port-matches', (this as any).address().port === server.address().port);
  res.end('ok');
}).listen(0, function () {
  // Pre-fix this line printed `undefined` (callback ran before assignment).
  console.log('cb-sees-server', typeof server);
  const addr = server.address();
  console.log('addr', typeof addr, addr.family, typeof addr.port, addr.port > 0);
  // `this` inside the listen callback is also the server.
  console.log('cb-this-port-matches', (this as any).address().port === addr.port);
  http.get({ port: addr.port, path: '/' }, (res: any) => {
    let body = '';
    res.on('data', (c: any) => { body += c; });
    res.on('end', () => {
      console.log('body', body);
      server.close();
    });
  });
});
// Must print BEFORE the listen callback: the emit is deferred to a later tick.
console.log('sync-after-listen');
"#,
    );
    assert_eq!(
        stdout,
        "sync-after-listen\n\
         cb-sees-server object\n\
         addr object IPv4 number true\n\
         cb-this-port-matches true\n\
         handler-this-port-matches true\n\
         body ok\n",
        "listen callback must fire post-tick with `this` = server, and \
         server.address() must expose the bound ephemeral port"
    );
}

/// `'listening'` listeners registered AFTER `listen()` returned must still
/// fire (the emit happens on a later tick, as in Node), and listeners
/// registered before `listen()` fire ahead of the listen callback.
#[test]
fn late_listening_listener_fires() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const http = require('http');
const server = http.createServer();
server.on('listening', () => console.log('pre-listener'));
server.listen(0, () => {
  console.log('listen-cb');
});
server.on('listening', () => {
  console.log('late-listener');
  server.close();
});
console.log('sync-tail');
"#,
    );
    assert_eq!(
        stdout, "sync-tail\npre-listener\nlisten-cb\nlate-listener\n",
        "'listening' must emit once on a later tick, in Node's listener order \
         (pre-registered, listen callback, late-registered)"
    );
}
