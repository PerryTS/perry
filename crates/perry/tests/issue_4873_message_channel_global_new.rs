//! Regression test for #4873: bare `new MessageChannel()` as a *global*
//! constructor must (a) link standalone — previously it emitted a call to the
//! stdlib-only `js_worker_threads_message_channel_new`, leaving an undefined
//! symbol unless something else in the graph imported `node:worker_threads` —
//! and (b) produce a real `{ port1, port2 }` object. React's scheduler runs
//! exactly this shape at module init (`typeof MessageChannel !== "undefined"`
//! then `new MessageChannel()`), so every React/ink app died here.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (link error = #4873 regression)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Standalone graph — no `worker_threads` import anywhere. Must link and the
/// channel must be a real object with port objects (Node prints `object
/// object object`).
#[test]
fn bare_new_message_channel_links_standalone() {
    let stdout = compile_and_run(
        r#"
const c = new MessageChannel();
console.log(typeof c, typeof c.port1, typeof c.port2);
if (typeof MessageChannel !== "undefined") {
  const channel = new MessageChannel();
  const port = channel.port2;
  console.log("scheduler-branch", typeof port.postMessage);
}
const g = new globalThis.MessageChannel();
console.log("globalThis-form", typeof g.port1, typeof g.port2);
const bc = new BroadcastChannel("chan");
console.log("broadcast", typeof bc, bc.name);
"#,
    );
    assert_eq!(
        stdout,
        "object object object\nscheduler-branch function\nglobalThis-form object object\nbroadcast object chan\n",
        "global MessageChannel/BroadcastChannel `new` must produce real objects"
    );
}

/// Graph that *does* import `node:worker_threads`: the global constructor
/// must delegate to the registered worker_threads factory, so paired-port
/// message delivery (`receiveMessageOnPort`) works on a channel created via
/// the bare global form.
#[test]
fn bare_new_message_channel_delegates_to_worker_threads() {
    let stdout = compile_and_run(
        r#"
import { receiveMessageOnPort } from "node:worker_threads";
const c = new MessageChannel();
c.port1.postMessage({ n: 7 });
const received = receiveMessageOnPort(c.port2);
console.log(JSON.stringify(received));
c.port1.close();
c.port2.close();
"#,
    );
    assert_eq!(
        stdout, "{\"message\":{\"n\":7}}\n",
        "global-form channel must use the real worker_threads ports when the module is linked"
    );
}
