//! Node treats `AbortSignal` as an `EventTarget`, so the module-level
//! `node:events` helpers accept one:
//!
//! ```js
//! const { setMaxListeners } = require("node:events");
//! setMaxListeners(50, controller.signal);   // how SDKs silence
//!                                           // MaxListenersExceededWarning on a
//!                                           // shared signal
//! ```
//!
//! Perry represents `AbortSignal` as its own native object (url/abort.rs) that
//! the events helpers' `event_helper_target` dispatch didn't recognize, so
//! `setMaxListeners(n, signal)` threw
//! `ERR_INVALID_ARG_TYPE: The "eventTargets" argument must be an instance of
//! EventEmitter or EventTarget. Received an instance of Object` — rejecting the
//! caller's whole request path in real applications.
//!
//! The helpers now recognize signals: `setMaxListeners` accepts them (a
//! faithful no-op — the warning threshold is the call's only Node-observable
//! effect and Perry never emits that warning for signals), `getMaxListeners`
//! reports the EventTarget default, and `listenerCount` /
//! `getEventListeners` report the signal's registered "abort" listeners.

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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary exited non-zero: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

#[test]
fn events_helpers_accept_abort_signal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
import { setMaxListeners, getMaxListeners, listenerCount, getEventListeners } from "node:events";

const c = new AbortController();

// The wall: must not throw ERR_INVALID_ARG_TYPE.
setMaxListeners(50, c.signal);
process.stdout.write("set=ok");

// EventTarget default cap is reported.
process.stdout.write(" max=" + getMaxListeners(c.signal));

// No listeners registered yet.
process.stdout.write(" count0=" + listenerCount(c.signal, "abort"));

// Register one "abort" listener; count and list reflect it, other event
// names stay empty.
const onAbort = () => {};
c.signal.addEventListener("abort", onAbort);
process.stdout.write(" count1=" + listenerCount(c.signal, "abort"));
process.stdout.write(" list1=" + getEventListeners(c.signal, "abort").length);
process.stdout.write(" other=" + getEventListeners(c.signal, "close").length);

// Regular emitters still validate: a plain object still throws.
let threw = false;
try {
  setMaxListeners(5, {} as any);
} catch {
  threw = true;
}
process.stdout.write(" plainThrows=" + threw + "\n");
"#,
    );
    assert_eq!(
        out, "set=ok max=10 count0=0 count1=1 list1=1 other=0 plainThrows=true",
        "events helpers on AbortSignal"
    );
}

#[test]
fn abort_signal_methods_dispatch_on_dynamic_receiver() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
// The statically-typed form (`c.signal.addEventListener(...)`) lowers to the
// native call directly. Minified SDK code stores the signal in an UNTYPED
// local, so the method resolves through dynamic property/method dispatch —
// which returned undefined and threw `addEventListener is not a function`.
const c = new AbortController();
const s: any = c.signal;

process.stdout.write("typeof=" + typeof s.addEventListener);

let fired = 0;
const onAbort = () => { fired++; };
s.addEventListener("abort", onAbort, { once: true }); // 3-arg SDK shape
s.addEventListener("abort", () => { fired += 10; });  // 2-arg shape

// removeEventListener through the dynamic receiver too.
const removed = () => { fired += 100; };
s.addEventListener("abort", removed);
s.removeEventListener("abort", removed);

// throwIfAborted: no-op while pending.
s.throwIfAborted();
process.stdout.write(" preAbortFired=" + fired);

c.abort();
process.stdout.write(" postAbortFired=" + fired);

// throwIfAborted now throws.
let threw = false;
try { s.throwIfAborted(); } catch { threw = true; }
process.stdout.write(" throwIfAborted=" + threw + "\n");
"#,
    );
    assert_eq!(
        out, "typeof=function preAbortFired=0 postAbortFired=11 throwIfAborted=true",
        "dynamic AbortSignal method dispatch"
    );
}
