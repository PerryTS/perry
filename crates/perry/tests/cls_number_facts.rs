//! CLS D2/D3: Numbers proven by the whole-program facts.
//!
//! * a closed method call whose every body returns a Number, and `+` over
//!   it and primitive-kind field reads, lowers to `fadd`: no
//!   `js_dynamic_string_or_number_add` in `describe`;
//! * the same program with a subclass returning a string, or a field that
//!   becomes a string, keeps the dynamic add (and node's output);
//! * `s.x = v` on a facts-proven nullable receiver is one receiver test and
//!   a slot store, not the by-name store site.

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

const SHAPES: &str = r#"
class Shape {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  area(): number { return 0; }
  describe(): number { return this.area() + this.x + this.y; }
}
class Circle extends Shape {
  r: number;
  constructor(x: number, y: number, r: number) { super(x, y); this.r = r; }
  area(): number { return Math.PI * this.r * this.r; }
}
class Rect extends Shape {
  w: number; h: number;
  constructor(x: number, y: number, w: number, h: number) { super(x, y); this.w = w; this.h = h; }
  area(): number { return this.w * this.h; }
}
const shapes: Shape[] = [new Circle(1, 2, 3), new Rect(4, 5, 6, 7)];
let total: number = 0;
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < shapes.length; i++) {
    const s: Shape = shapes[i];
    s.x = (s.x + 1) % 10;
    total += s.describe();
  }
}
console.log(total.toFixed(3));
"#;

#[test]
fn proven_number_returns_add_without_the_dynamic_helper() {
    let dir = tempfile::tempdir().unwrap();
    let (ir, out) = compile_and_run(dir.path(), SHAPES, false);
    let describe = body(&ir, "perry_method_main_ts__Shape__describe");
    assert!(
        !describe.contains("js_dynamic_string_or_number_add"),
        "describe keeps the dynamic add:\n{describe}"
    );
    assert!(describe.contains("fadd double"), "describe has no fadd");
    // The store into the proven slot has no by-name site.
    assert!(
        ir.contains("wpf.set.obj"),
        "the main loop's s.x store is not the proven store"
    );
    let dir2 = tempfile::tempdir().unwrap();
    let node = Command::new("node")
        .arg("--experimental-strip-types")
        .arg({
            let p = dir2.path().join("m.ts");
            std::fs::write(&p, SHAPES).unwrap();
            p
        })
        .output();
    if let Ok(node) = node {
        if node.status.success() {
            assert_eq!(out, String::from_utf8_lossy(&node.stdout));
        }
    }
}

#[test]
fn a_subclass_returning_a_string_keeps_the_dynamic_add() {
    let source = format!(
        "{SHAPES}\nclass Odd extends Shape {{ area(): number {{ return \"7\" as any; }} }}\nconsole.log(new Odd(1, 2).describe());\n"
    );
    let dir = tempfile::tempdir().unwrap();
    let (ir, out) = compile_and_run(dir.path(), &source, false);
    let describe = body(&ir, "perry_method_main_ts__Shape__describe");
    assert!(
        describe.contains("js_dynamic_string_or_number_add")
            || describe.contains("guarded_add.dynamic"),
        "describe lost its dynamic add although a body returns a string"
    );
    assert!(out.ends_with("712\n"), "wrong output: {out}");
}

#[test]
fn a_field_that_becomes_a_string_keeps_the_dynamic_add() {
    let source =
        format!("{SHAPES}\n(shapes[0] as any).y = \"y\";\nconsole.log(shapes[0].describe());\n");
    let dir = tempfile::tempdir().unwrap();
    let (ir, out) = compile_and_run(dir.path(), &source, false);
    let describe = body(&ir, "perry_method_main_ts__Shape__describe");
    assert!(
        describe.contains("js_dynamic_string_or_number_add")
            || describe.contains("guarded_add.dynamic"),
        "describe lost its dynamic add although a field holds a string"
    );
    assert!(
        out.lines().last().unwrap().contains('y'),
        "wrong output: {out}"
    );
}
