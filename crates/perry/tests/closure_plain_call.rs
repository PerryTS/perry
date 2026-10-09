//! A plain closure call is one check and a jump.
//!
//! A compiled body that bundles no rest or `arguments` array is PLAIN: its
//! `JsFunctionInfo` records `plain_params`, and `js_closure_call{N}` hands a
//! call passing at least that many arguments straight to the body with
//! `become`, leaving no frame behind. Everything else (a bound value, a rest
//! or `arguments` body, a short call, a runtime native, a value that is no
//! function) takes the dispatcher's ladder exactly as before.
//!
//! The fixture calls through function VALUES (`fs[i](...)`), so every call
//! reaches the runtime entry: every arity the per-arity entries take and the
//! wide array path past them, over- and under-application, rest and
//! `arguments` bodies, bound functions and methods, call/apply/Reflect.apply,
//! generators and async functions, a class constructor and non-callables, a
//! throw from a plain body caught by its caller, stack overflow through the
//! entry, and allocation under the calls. The expected output is node's. It
//! also runs under forced evacuation, so closures move while being called.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const FIXTURE: &str = r#"
// Plain calls: every arity the entries take, exact and over-applied.
function arityCalls(): string {
  const f0 = () => "z";
  const f1 = (a: any) => a;
  const f2 = (a: any, b: any) => a + b;
  const f3 = (a: any, b: any, c: any) => a + b + c;
  const f8 = (a: any, b: any, c: any, d: any, e: any, f: any, g: any, h: any) =>
    a + b + c + d + e + f + g + h;
  const f9 = (a: any, b: any, c: any, d: any, e: any, f: any, g: any, h: any, i: any) =>
    a + b + c + d + e + f + g + h + i;
  const f16 = (a: any, b: any, c: any, d: any, e: any, f: any, g: any, h: any,
    i: any, j: any, k: any, l: any, m: any, n: any, o: any, p: any) =>
    [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p].join("");
  const fs: any[] = [f0, f1, f2, f3, f8, f9, f16];
  const out: string[] = [];
  out.push(fs[0]());
  out.push(fs[1](1));
  out.push(fs[2](1, 2));
  out.push(fs[3](1, 2, 3));
  out.push(fs[4](1, 2, 3, 4, 5, 6, 7, 8));
  out.push(fs[5](1, 2, 3, 4, 5, 6, 7, 8, 9));
  out.push(fs[6]("a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p"));
  // Over-application: the extra arguments are ignored.
  out.push(fs[1](7, 8, 9));
  out.push(fs[2](1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11));
  out.push(fs[0](1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16));
  out.push(fs[0](1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17));
  // Extra arguments never widen a short body's dynamic-call ABI.
  const surplus: any[] = [];
  for (let i = 0; i < 2048; i++) surplus.push(i);
  out.push(fs[1](...surplus));
  // Under-application: the missing parameters are undefined.
  out.push(String(fs[3](1)));
  out.push(String(fs[5](1, 2)));
  return out.join(",");
}
console.log("arity", arityCalls());

// Rest and `arguments` bodies take the bundling arm.
function bundling(): string {
  const rest: any = (a: any, ...r: any[]) => a + ":" + r.length + ":" + r.join("|");
  const args: any = function () {
    return arguments.length + ":" + Array.prototype.join.call(arguments, "|");
  };
  const both: any = function (a: any, ...r: any[]) {
    return a + ":" + r.length + ":" + arguments.length;
  };
  const fs: any[] = [rest, args, both];
  return [fs[0](1), fs[0](1, 2, 3), fs[1](), fs[1](4, 5), fs[2](6), fs[2](6, 7, 8)].join(",");
}
console.log("bundling", bundling());

// Bound functions and bound methods take their arm, with the bound receiver
// and the bound arguments first.
function bound(): string {
  function who(this: any, a: any, b: any) {
    return (this && this.name) + ":" + a + ":" + b;
  }
  const o = { name: "o", m(this: any, x: any) { return this.name + x; } };
  const b0: any = who.bind({ name: "b" });
  const b1: any = who.bind({ name: "c" }, 1);
  const bm: any = o.m.bind(o);
  const bb: any = b1.bind({ name: "ignored" }, 2);
  const fs: any[] = [b0, b1, bm, bb];
  return [fs[0](1, 2), fs[1](3), fs[1](), fs[2]("!"), fs[3](), fs[3](9)].join(",");
}
console.log("bound", bound());

// call / apply / Reflect.apply pass an explicit receiver.
function explicitThis(): string {
  function get(this: any, k: any) {
    return this[k];
  }
  const fs: any[] = [get];
  const r = { a: 1, b: 2 };
  return [fs[0].call(r, "a"), fs[0].apply(r, ["b"]), Reflect.apply(fs[0], r, ["a"])].join(",");
}
console.log("this", explicitThis());

// Generators and async functions are plain bodies: the body builds the
// generator or the promise.
function* gen(a: number, b: number) {
  yield a;
  yield b;
}
async function later(x: number) {
  return x * 2;
}
const builders: any[] = [gen, later];
console.log("gen", [...builders[0](1, 2)].join(","), [...builders[0](1, 2, 3)].join(","));

// A class constructor is not callable without `new`.
class K {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
const ctors: any[] = [K];
try {
  ctors[0](1);
  console.log("ctor", "called");
} catch (e: any) {
  console.log("ctor", e instanceof TypeError);
}

// A non-callable value still throws a TypeError.
const notFns: any[] = [{}, 5, "s", null];
for (const v of notFns) {
  try {
    v(1);
  } catch (e: any) {
    console.log("notfn", e instanceof TypeError);
  }
}

// A throw from a plain body reaches the caller's catch through the entry.
function thrower(n: number): number {
  if (n > 2) throw new Error("boom" + n);
  return n;
}
const throwers: any[] = [thrower];
function catchThrough(): string {
  const out: string[] = [];
  for (let i = 0; i < 5; i++) {
    try {
      out.push(String(throwers[0](i)));
    } catch (e: any) {
      out.push(e.message);
    }
  }
  return out.join(",");
}
console.log("throw", catchThrough());

// Deep recursion through closure values: the stack guard still throws a
// catchable RangeError.
const deep: any[] = [];
deep.push((n: number): number => deep[0](n + 1) + 1);
try {
  deep[0](0);
  console.log("deep", "no overflow");
} catch (e: any) {
  console.log("deep", e instanceof RangeError);
}

// Closures allocate while called through the entry: the collector runs
// under the calls and the results stay intact.
function churn(): number {
  const mk = (i: number) => (x: number) => ({ v: x + i, pad: [i, i, i] });
  const fs: any[] = [];
  for (let i = 0; i < 64; i++) fs.push(mk(i));
  let s = 0;
  for (let r = 0; r < 2000; r++) {
    for (let i = 0; i < 64; i++) s += fs[i](r).v - r;
  }
  return s;
}
console.log("churn", churn());

builders[1](21).then((v) => console.log("async", v));
"#;

/// node v26 on the fixture.
const EXPECTED: &str = "arity z,1,3,6,36,45,abcdefghijklmnop,7,3,z,z,0,NaN,NaN\n\
bundling 1:0:,1:2:2|3,0:,2:4|5,6:0:1,6:2:3\n\
bound b:1:2,c:1:3,c:1:undefined,o!,c:1:2,c:1:2\n\
this 1,2,1\n\
gen 1,2 1,2\n\
ctor true\n\
notfn true\n\
notfn true\n\
notfn true\n\
notfn true\n\
throw 0,1,2,boom3,boom4\n\
deep true\n\
churn 4032000\n\
async 42\n\
";

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn compile(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let bin = dir.join("main_bin");
    std::fs::write(&entry, FIXTURE).expect("write fixture");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg("--no-auto-optimize")
        .arg(&entry)
        .arg("-o")
        .arg(&bin)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_GC_INSTRUMENTS", "1")
        .env(
            "PERRY_RUNTIME_DIR",
            perry_bin().parent().expect("runtime directory"),
        )
        .output()
        .expect("run perry compile");
    assert_success("perry compile", &compile);
    bin
}

fn run(bin: &Path, dir: &Path, env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run compiled fixture");
    assert_success(&format!("compiled fixture {env:?}"), &out);
    if env.iter().any(|(key, _)| *key == "PERRY_GC_FORCE_EVACUATE") {
        let diagnostics = String::from_utf8_lossy(&out.stderr);
        assert!(
            diagnostics.split_whitespace().any(|word| {
                word.strip_prefix("copied_objects=")
                    .and_then(|count| count.parse::<u64>().ok())
                    .is_some_and(|count| count > 0)
            }),
            "forced evacuation must actually move objects:\n{diagnostics}"
        );
    }
    String::from_utf8(out.stdout).expect("utf-8")
}

#[test]
fn plain_closure_calls_match_node_on_every_call_shape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(dir.path());
    assert_eq!(run(&bin, dir.path(), &[]), EXPECTED, "plain run");
    assert_eq!(
        run(
            &bin,
            dir.path(),
            &[
                ("PERRY_GC_FORCE_EVACUATE", "1"),
                ("PERRY_GC_VERIFY_EVACUATION", "1"),
                ("PERRY_GC_DIAG", "1"),
            ],
        ),
        EXPECTED,
        "forced evacuation"
    );
}
