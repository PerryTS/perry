//! Regression test for #5129: a `Proxy` whose `set` trap calls the 4-arg
//! `Reflect.set(target, key, value, receiver)` (forwarding the proxy itself as
//! the receiver) segfaulted (SIGSEGV / exit 139).
//!
//! Root cause: `Reflect.set` → `OrdinarySetWithOwnDescriptor` ends in
//! `CreateDataProperty(Receiver, P, V)` = `Receiver.[[DefineOwnProperty]]`.
//! Perry's `create_or_update_receiver_property` instead did an ordinary data
//! store (`target_set`) on the receiver — but the receiver was a Proxy (a small
//! registered id, not a heap object), so it dereferenced a fake pointer.
//!
//! Fix: when the receiver is a Proxy, route through its `[[DefineOwnProperty]]`
//! (`js_reflect_define_property` with a CreateDataProperty descriptor), which
//! invokes the `defineProperty` trap or, absent one, defines on the target.

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
        // Bypass the compile cache so the test always exercises the current
        // proxy-receiver codegen/runtime path rather than a stale artifact.
        .arg("--no-cache")
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
        "compiled binary failed (pre-fix: SIGSEGV / exit 139)\nstatus: {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn proxy_set_trap_reflect_set_with_receiver_does_not_segfault() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const p = new Proxy({ a: 1 } as Record<string, unknown>, {
  set(t, k, v, r) { return Reflect.set(t, k, v, r); },
});
p.b = 2;
console.log("set ok");
console.log(p.a, p.b);

// The trap body runs once per write, then the define lands on the target.
const log: string[] = [];
const p2 = new Proxy({ x: 0 } as Record<string, unknown>, {
  set(t, k, v, r) { log.push("trap:" + String(k)); return Reflect.set(t, k, v, r); },
});
p2.x = 9; p2.y = 7;
console.log(log.join(","), p2.x, p2.y);

// A defineProperty trap on the same proxy must be invoked by the receiver define.
const dlog: string[] = [];
const p3 = new Proxy({} as Record<string, unknown>, {
  set(t, k, v, r) { return Reflect.set(t, k, v, r); },
  defineProperty(t, k, d) { dlog.push("def:" + String(k)); return Reflect.defineProperty(t, k, d); },
});
p3.z = 5;
console.log(dlog.join(","), p3.z);
"#,
    );
    assert_eq!(stdout, "set ok\n1 2\ntrap:x,trap:y 9 7\ndef:z 5\n");
}
