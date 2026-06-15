//! Regression test for #5138: a `super.<method>` value stored on an instance
//! and later invoked method-style ran the base method with `this=undefined`.
//!
//! The canonical real-world shape is rxjs's `OperatorSubscriber`, which does
//! `this._complete = onComplete ? wrapper : super._complete` in its
//! constructor. With no override callback the operator forwards completion to
//! the base `Subscriber`, so `map(...)` relies on `super._complete`. A
//! synchronous `of(...).pipe(filter, map, reduce)` then never drove its
//! subscriber to completion: `reduce` only emits on `complete`, `firstValueFrom`
//! never resolved, and the program exited 13 ("Detected unsettled top-level
//! await").
//!
//! Root cause: the class-method closure wrapper emitted for value-form
//! `super.<method>` (`__perry_wrap_<method>`) hardcoded `this=undefined` when
//! forwarding to the underlying method. That is correct for a *bare* call
//! (`const fn = super.greet; fn()` → strict `this`), but wrong for a
//! *method-style* call where the dispatcher has already set IMPLICIT_THIS to the
//! receiver. Fix: the wrapper now reads IMPLICIT_THIS and forwards it as `this`,
//! so both call shapes match Node.

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

/// `this._x = super._x; obj._x()` must run the base method with `this === obj`.
/// Pre-fix the base method saw `this=undefined` and threw on `this.tag`.
#[test]
fn super_method_value_invoked_method_style_binds_receiver() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Base {
  _complete() { console.log('base complete', this.tag); }
}
class Child extends Base {
  tag = 'child';
  _complete: () => void;
  constructor(onComplete?: () => void) {
    super();
    this._complete = onComplete ? onComplete : super._complete;
  }
}
const c = new Child();
c._complete();
"#,
    );
    assert_eq!(stdout, "base complete child\n");
}

/// The other side of the contract: a *bare* call of a `super.<method>` value
/// must still see strict-mode `this === undefined` (Node parity), not leak a
/// stale receiver.
#[test]
fn super_method_value_invoked_bare_keeps_undefined_this() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Base {
  greet() { console.log('this is', this === undefined ? 'undefined' : 'defined'); }
}
class Child extends Base {
  run() { const fn = super.greet; fn(); }
}
new Child().run();
"#,
    );
    assert_eq!(stdout, "this is undefined\n");
}
