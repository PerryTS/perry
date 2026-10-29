//! CLS D1: a receiver with a static class (an annotation, or `this` inside a
//! method) takes the facts-proven class-id switch, exactly as an untyped
//! receiver does. The annotation only picks the lowering; the facts prove the
//! semantics, so a site the facts do not prove keeps the guarded typed path.
//!
//! * proven: the annotated call in the loop and `this.area()` inside
//!   `describe` are a class-id switch of direct calls (`idispatch.case`),
//!   with no `method_direct` guard and no by-id native dispatch;
//! * not proven (prototype method replaced, own override, setPrototypeOf, a
//!   value that is not the annotated class): no switch, the guarded typed
//!   path, and node's output.

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

/// Compile `source` (whole program when `whole`); return the saved LLVM IR of
/// the entry module and the program's stdout.
fn compile_and_run(dir: &Path, source: &str, whole: bool) -> (String, String) {
    let src = dir.join("main.ts");
    std::fs::write(&src, source).unwrap();
    let exe = dir.join("main_bin");
    let ll_dir = dir.join(if whole { "ll_w" } else { "ll_u" });
    std::fs::create_dir_all(&ll_dir).unwrap();
    let mut cmd = Command::new(perry_bin());
    clean(&mut cmd)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_SAVE_LL", &ll_dir);
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
    let ll = std::fs::read_to_string(ll_dir.join("main_ts.ll")).expect("saved main_ts.ll");
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed: {:?}", ran.status);
    (ll, String::from_utf8_lossy(&ran.stdout).into_owned())
}

/// The body of the function defined as `@<symbol>(` (its `define` line, not a
/// call of it).
fn body<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let head = format!("@{symbol}(");
    let at = ir
        .match_indices("define ")
        .map(|(i, _)| i)
        .find(|&i| {
            ir[i..]
                .lines()
                .next()
                .is_some_and(|line| line.contains(&head))
        })
        .unwrap_or_else(|| panic!("no function {symbol}"));
    let end = ir[at..].find("\n}\n").map_or(ir.len(), |e| at + e);
    &ir[at..end]
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("perry_cls_d1_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const PROVEN: &str = r#"
class Shape {
  x: number = 1;
  area(): number { return 0; }
  describe(): number { return this.area() + this.x; }
}
class Circle extends Shape { area(): number { return 3; } }
class Rect extends Shape { area(): number { return 4; } }
const shapes: Shape[] = [new Circle(), new Rect(), new Circle(), new Rect()];
function total(): number {
  let t = 0;
  for (let i = 0; i < shapes.length; i++) {
    const s: Shape = shapes[i];
    t += s.describe();
  }
  return t;
}
console.log(total());
"#;

#[test]
fn annotated_and_this_receivers_use_the_proven_class_switch() {
    let d = tmp("proven");
    for whole in [true, false] {
        let (ll, out) = compile_and_run(&d, PROVEN, whole);
        assert_eq!(out, "18\n", "whole={whole}");
        for (what, f) in [
            ("main loop s.describe()", "perry_fn_main_ts__total"),
            (
                "this.area() in describe",
                "perry_method_main_ts__Shape__describe",
            ),
        ] {
            let b = body(&ll, f);
            assert!(
                b.contains("idispatch.case"),
                "{what} (whole={whole}) is a class-id switch:\n{b}"
            );
            for gone in ["method_direct.guard", "js_native_call_method_by_id"] {
                assert!(
                    !b.contains(gone),
                    "{what} (whole={whole}) keeps no `{gone}`:\n{b}"
                );
            }
        }
    }
}

const TOTAL: &str = "perry_fn_main_ts__total";
const DESCRIBE: &str = "perry_method_main_ts__Shape__describe";

/// Each: (name, source, node's output, functions whose call sites the facts
/// do not prove). A write the census sees, or a value of another class, means
/// the typed path stays at those sites and the result is node's.
const UNPROVEN: [(&str, &str, &str, &[&str]); 4] = [
    (
        "method_replaced",
        include_str!("../../../test-files/test_gap_cls_method_replaced.ts"),
        "18\n32\n32\n",
        // `area` is replaced on a prototype: `this.area()` cannot switch.
        &[DESCRIBE],
    ),
    (
        "own_override",
        include_str!("../../../test-files/test_gap_cls_own_override.ts"),
        "13\n110\n104\n",
        &[TOTAL, DESCRIBE],
    ),
    (
        "set_prototype_of",
        include_str!("../../../test-files/test_gap_cls_set_prototype_of.ts"),
        "13\n14\n510\n",
        &[TOTAL, DESCRIBE],
    ),
    (
        "lying_annotation",
        include_str!("../../../test-files/test_gap_cls_lying_annotation.ts"),
        "2079\n70 7\n2000\n",
        // The loop holds values of other classes behind the annotation.
        &[TOTAL],
    ),
];

#[test]
fn receivers_the_facts_do_not_prove_keep_the_typed_path() {
    for (name, source, expected, unproven) in UNPROVEN {
        let d = tmp(name);
        for whole in [true, false] {
            let (ll, out) = compile_and_run(&d, source, whole);
            assert_eq!(out, expected, "{name} whole={whole}");
            for f in unproven {
                let b = body(&ll, f);
                assert!(
                    !b.contains("idispatch.case"),
                    "{name} (whole={whole}): an unproven receiver in {f} must not switch:\n{b}"
                );
            }
        }
    }
}
