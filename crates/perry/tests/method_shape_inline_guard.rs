//! Regression coverage for the inlined monomorphic method-shape guard.
//!
//! The direct call is emitted in `invoke.ts`, while `main.ts` installs an own
//! method through an alias the callee module cannot see statically. The inline
//! guard must therefore re-check the live ShapeId on every call. The fixture
//! also proves an unrelated descriptor does not poison the site before an own
//! method replacement changes the receiver shape and reaches dynamic dispatch.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn write_fixture(root: &Path) -> PathBuf {
    std::fs::write(
        root.join("counter.ts"),
        r#"
export class Counter {
  value(): number { return 1 }
}
"#,
    )
    .expect("write counter module");
    std::fs::write(
        root.join("invoke.ts"),
        r#"
import { Counter } from "./counter"

export function invoke(counter: Counter): number {
  return counter.value()
}
"#,
    )
    .expect("write guarded caller module");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { Counter } from "./counter"
import { invoke } from "./invoke"

const own: any = new Counter()
console.log("base:", invoke(own))

const unrelated: any = {}
Object.defineProperty(unrelated, "locked", { value: 1, writable: false })
console.log("unrelated descriptor:", invoke(own))

own.value = () => 7
console.log("own:", invoke(own))
"#,
    )
    .expect("write entry module");
    entry
}

#[test]
fn live_shape_change_deopts_to_dynamic_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = write_fixture(dir.path());
    let binary = dir.path().join("method_shape_guard");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&binary).output().expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "base: 1\nunrelated descriptor: 1\nown: 7\n"
    );
}
