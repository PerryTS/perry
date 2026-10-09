//! Every closure is born with every cell capture slot filled.
//!
//! A binding a closure captures and someone mutates lives in a GC cell (a box
//! or a scope object), and the closure's slot for it holds the cell. Closure
//! birth (`perry-codegen/src/stmt/binding_cell.rs`) gives the binding its cell
//! when its declaring statement did not run on the birth path, so a closure
//! body reads its cells with plain loads and never validates them at entry.
//!
//! The fixture covers every closure kind (arrow, function expression, nested
//! function declaration, object-literal method and accessor, closure inside a
//! class method, generator, async) plus TDZ, scope groups, per-iteration cells,
//! transitive captures and births on a path that skipped the declaration. It
//! is compiled with `PERRY_ASSERT_CAPTURE_CELLS=1`, which makes every birth
//! check each cell word it installs and abort on one that is not a live cell:
//! running the program proves the rule for every birth it executes. It also
//! runs under forced evacuation, so births and capture reads happen while
//! cells relocate.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const FIXTURE: &str = r#"
// Arrow recursion through a captured const: the cell exists before the
// initializer runs (TDZ box), and every call enters through it.
function mkSelf() {
  const self = (n: number): number => (n < 2 ? n : self(n - 1) + self(n - 2));
  return self;
}
console.log("self", mkSelf()(15));

// TDZ: a closure born before its binding is initialized throws on read, and
// reads the value once the declaration ran.
function tdz(): string {
  const read = () => x;
  let before: string;
  try {
    before = String(read());
  } catch (e) {
    before = e instanceof ReferenceError ? "ReferenceError" : "other";
  }
  let x = 41;
  x = x + 1;
  return before + "/" + read();
}
console.log("tdz", tdz());

// A scope group: two mutated bindings captured by the same closures.
function group(n: number): string {
  let a = 0;
  let b = 1;
  const step = () => {
    const t = a + b;
    a = b;
    b = t;
  };
  const peek = () => a + ":" + b;
  for (let i = 0; i < n; i++) step();
  return peek();
}
console.log("group", group(10));

// A hoisted nested declaration capturing a later `let`, and a function
// expression reading it more than once.
function hoisted(): string {
  function bump() {
    count = count + 1;
    return count;
  }
  let count = 10;
  const fe = function () {
    let s = 0;
    for (let i = 0; i < 2; i++) s += count;
    return s;
  };
  bump();
  bump();
  return bump() + "," + fe();
}
console.log("hoisted", hoisted());

// Object-literal method and accessor capturing a mutated binding.
function counterObj(): number {
  let n = 0;
  const o = {
    inc() {
      n++;
      return n;
    },
    get value() {
      return n + n;
    },
  };
  o.inc();
  o.inc();
  return o.value;
}
console.log("objmethod", counterObj());

// Closures born inside a class method.
class Acc {
  total = 0;
  run(xs: number[]): number {
    let sum = 0;
    xs.forEach((x) => {
      sum += x;
    });
    const read = () => sum + sum;
    this.total = read();
    return this.total;
  }
}
console.log("classmethod", new Acc().run([1, 2, 3, 4]));

// Generator and async bodies capturing mutated bindings.
function* gen() {
  let k = 0;
  const inc = () => {
    k += 2;
    return k;
  };
  while (k < 6) yield inc();
}
console.log("gen", [...gen()].join(","));

async function asyncCap(): Promise<number> {
  let v = 1;
  const dbl = () => {
    v = v * 2;
    return v;
  };
  await null;
  dbl();
  await null;
  return dbl();
}

// Births in a switch case and after a branch, away from the declaration.
function skipped(c: number): string {
  switch (c) {
    case 1:
      let x: any = 5;
      const inc = () => {
        x = x + 1;
        return x;
      };
      return "taken:" + inc();
    case 2: {
      const later = (k: boolean) => (k ? x + x : "skip");
      return "skipped:" + later(false);
    }
  }
  return "none";
}
console.log("switch", skipped(1), skipped(2), skipped(3));

function branch(flag: boolean): string {
  if (flag) {
    var v: any = 1;
  }
  const touch = () => {
    v = (v || 0) + 1;
    return v;
  };
  touch();
  return String(touch());
}
console.log("branch", branch(true), branch(false));

// Per-iteration cells: closures born in a loop each own their binding.
function perIter(): string {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    let j = i * 10;
    fs.push(() => ++j);
  }
  return fs.map((f) => f() + f()).join(",");
}
console.log("periter", perIter());

// A transitive capture: the inner closure is born from the outer's slot.
function nested(): number {
  let z = 1;
  const outer = () => {
    const inner = () => {
      z = z + 1;
      return z;
    };
    return inner() + inner();
  };
  return outer() + z;
}
console.log("nested", nested());

// Many births between allocations: every cell must survive relocation.
function gcBirth(): number {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 20000; i++) {
    let c = i;
    const junk = { a: [i, i + 1], s: "x" + i };
    fs.push(() => {
      c = c + junk.a.length;
      return c;
    });
  }
  let s = 0;
  for (const f of fs) s += f();
  return s;
}
console.log("gc", gcBirth());

// A closure whose body declares a function with cells of its own: the
// closure's capture list names those cells, it has no storage for them, so its
// birth mints them, two cells, the first held across the second's allocation.
function mintTwo(seed: number) {
  const o = { v: seed };
  const f = () => {
    function inner() {
      let a = o.v;
      const g = () => {
        a++;
      };
      let b = 1;
      const h = () => {
        b++;
      };
      g();
      h();
      return a + b;
    }
    return inner();
  };
  return f;
}
function mintBirths(): number {
  let s = 0;
  const keep: (() => number)[] = [];
  for (let i = 0; i < 50000; i++) {
    const f = mintTwo(i);
    if (i % 100 === 0) keep.push(f);
    s += f();
  }
  for (const f of keep) s += f();
  return s;
}
console.log("mint", mintBirths());

asyncCap().then((r) => console.log("async", r));
"#;

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn compile(dir: &Path) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let bin = dir.join("main_bin");
    std::fs::write(&entry, FIXTURE).expect("write fixture");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&bin)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1")
        .env("PERRY_ASSERT_CAPTURE_CELLS", "1")
        .output()
        .expect("run perry compile");
    assert_success("perry compile", &compile);
    let stderr = String::from_utf8_lossy(&compile.stderr).into_owned();
    let ir_path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    let ir = std::fs::read_to_string(ir_path).expect("read kept LLVM IR");
    (bin, ir)
}

/// CALL sites of `name`, not its `declare` line.
fn call_count(ir: &str, name: &str) -> usize {
    let needle = format!("@{name}(");
    ir.lines()
        .filter(|l| l.contains(&needle) && (l.contains("call ") || l.contains("invoke ")))
        .count()
}

/// The bodies of every emitted function whose symbol contains `name`
/// (specialised clones included).
fn function_bodies<'a>(ir: &'a str, name: &str) -> Vec<Vec<&'a str>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<&str>> = None;
    for line in ir.lines() {
        if line.starts_with("define") {
            cur = line.contains(name).then(Vec::new);
        }
        if let Some(body) = cur.as_mut() {
            body.push(line);
            if line == "}" {
                out.push(cur.take().unwrap());
            }
        }
    }
    out
}

/// The register an IR line defines (`%r12 = ...`).
fn defined_reg(line: &str) -> Option<&str> {
    let reg = line.trim_start().strip_prefix('%')?.split_once(" = ")?.0;
    Some(reg)
}

/// A mint allocates, so it can collect: every word a minting birth hands the
/// closure allocation must be produced (re-read from its root) at or below
/// the last mint. A word still in a register from above a mint names a cell or value
/// that collection may have moved or reclaimed, and no statepoint names it.
fn assert_minting_birth_words_are_rooted(body: &[&str]) {
    let text = body.join("\n");
    let mints: Vec<usize> = body
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("@js_box_alloc_bits(") || l.contains("@js_scope_alloc("))
        .map(|(i, _)| i)
        .collect();
    assert!(
        mints.len() >= 2,
        "premise: the birth mints two cells:\n{text}"
    );
    let last_mint = *mints.last().unwrap();
    let alloc = body[last_mint..]
        .iter()
        .position(|l| l.contains("@js_closure_alloc_init_boxed("))
        .map(|i| i + last_mint)
        .unwrap_or_else(|| panic!("premise: a bulk boxed birth below the mints:\n{text}"));
    let words: Vec<&str> = body[last_mint..alloc]
        .iter()
        .filter_map(|l| l.trim_start().strip_prefix("store i64 %"))
        .map(|rest| rest.split(',').next().unwrap().trim())
        .collect();
    // `o`, both cells, and the inner closures' own capture names.
    assert!(
        words.len() >= 3,
        "the capture words reach the buffer:\n{text}"
    );
    for w in words {
        let def = body
            .iter()
            .position(|l| defined_reg(l) == Some(w))
            .unwrap_or_else(|| panic!("no definition of %{w}:\n{text}"));
        // The last mint's own cell is the one word that needs no root.
        assert!(
            def >= last_mint,
            "capture word %{w} is produced above the last mint and reaches the \
             closure from a register:\n{text}"
        );
    }
}

fn run(bin: &Path, dir: &Path, env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run compiled fixture");
    assert_success(&format!("compiled fixture {env:?}"), &out);
    String::from_utf8(out.stdout).expect("utf-8")
}

#[test]
fn every_closure_birth_fills_every_cell_capture_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, ir) = compile(dir.path());

    // The birth check is live: births of cell-capturing closures carry it.
    // (A check that is never emitted would pass every run below vacuously.)
    let asserts = call_count(&ir, "js_capture_cell_assert");
    assert!(
        asserts >= 12,
        "expected a birth check per installed cell, found {asserts}"
    );
    // A birth that mints holds its words rooted across every mint.
    // (Its ABI wrapper only forwards the call; the bodies are the births.)
    let minting: Vec<_> = function_bodies(&ir, "__mintTwo")
        .into_iter()
        .filter(|body| body.iter().any(|l| l.contains("@js_closure_alloc")))
        .collect();
    assert!(!minting.is_empty(), "premise: mintTwo's births are emitted");
    for body in &minting {
        assert_minting_birth_words_are_rooted(body);
    }
    // Closure entry no longer asks the runtime whether a capture is a cell.
    for gone in ["js_scope_capture_base", "js_box_capture_cell_ptr"] {
        assert!(
            !ir.contains(&format!("@{gone}(")),
            "{gone} must not be emitted or declared any more"
        );
    }

    let node = Command::new("node")
        .current_dir(dir.path())
        .arg("--experimental-strip-types")
        .arg("--no-warnings")
        .arg(dir.path().join("main.ts"))
        .output()
        .expect("run Node semantic oracle");
    assert_success("Node", &node);
    let expected = String::from_utf8(node.stdout).expect("utf-8");
    assert!(expected.contains("tdz ReferenceError/42"), "{expected}");

    assert_eq!(run(&bin, dir.path(), &[]), expected, "plain run");
    // Moving-collection witness: births and capture reads while cells move.
    assert_eq!(
        run(
            &bin,
            dir.path(),
            &[
                ("PERRY_GC_FORCE_EVACUATE", "1"),
                ("PERRY_GC_VERIFY_EVACUATION", "1"),
            ],
        ),
        expected,
        "forced evacuation"
    );
    // Collections at many points of the minting births.
    for mb in ["1", "2", "3"] {
        assert_eq!(
            run(
                &bin,
                dir.path(),
                &[
                    ("PERRY_GC_SCAVENGE_NURSERY_MB", mb),
                    ("PERRY_GC_FORCE_EVACUATE", "1"),
                    ("PERRY_GC_VERIFY_EVACUATION", "1"),
                ],
            ),
            expected,
            "nursery cap {mb} MB"
        );
    }
}
