//! A class relinked as another class's parent (`Object.setPrototypeOf(B,
//! A)`, `Reflect.setPrototypeOf`, `B.__proto__ = A`): `B`'s `super()` runs
//! `A`'s constructor on the `B` instance under construction. Per unit, the
//! relink is a statement unit of its own, so `A`'s unit and `B`'s unit must
//! both learn of it: `A`'s constructor stores into a receiver of `B`'s
//! layout, and `B`'s `super()` follows the live superclass. Both modes give
//! node's output.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Compile `source` (whole program when `whole`) and return its stdout.
fn compile_and_run(dir: &Path, source: &str, whole: bool) -> String {
    let src = dir.join("main.ts");
    std::fs::write(&src, source).unwrap();
    let exe = dir.join(if whole { "main_w" } else { "main_u" });
    let mut cmd = Command::new(perry_bin());
    clean(&mut cmd).current_dir(dir).env("PERRY_NO_CACHE", "1");
    if whole {
        cmd.env("PERRY_FACTS_UNITS", "0");
    }
    let out = cmd
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed (whole={whole})\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = clean(&mut Command::new(&exe)).output().expect("run");
    assert!(ran.status.success(), "run failed: {:?}", ran.status);
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

#[test]
fn a_relinked_parent_constructor_runs_on_the_subclass_instance() {
    let relinks = [
        "Object.setPrototypeOf(OrderedDefault, ActualParent);",
        "Reflect.setPrototypeOf(OrderedDefault, ActualParent);",
        "(OrderedDefault as any).__proto__ = ActualParent;",
    ];
    let ctors = ["", "constructor() { super(); }"];
    let dir = std::env::temp_dir().join(format!("perry_rlk_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for relink in relinks {
        for ctor in ctors {
            let source = format!(
                "class DeclaredParent {{}}\n\
                 class ActualParent {{ constructor() {{ this.parentSaw = Object.keys(this).join(\",\"); this.dynamic = 1; }} }}\n\
                 class OrderedDefault extends DeclaredParent {{ own = 2; {ctor} }}\n\
                 {relink}\n\
                 const o: any = new OrderedDefault();\n\
                 console.log(o.parentSaw, Object.keys(o).join(\",\"));\n\
                 const ap: any = new ActualParent();\n\
                 console.log(ap.parentSaw, ap.dynamic);\n"
            );
            for whole in [false, true] {
                assert_eq!(
                    compile_and_run(&dir, &source, whole),
                    " parentSaw,dynamic,own\n 1\n",
                    "whole={whole}\n{source}"
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
