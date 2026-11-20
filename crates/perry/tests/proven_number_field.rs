//! A field the whole-program facts prove only ever holds Numbers (F2 is
//! exactly Number) reads as a Number. A Number has one encoding in every JS
//! value (the raw double), so the read is the slot load feeding the `fmul`:
//! no tag test, no select, no branch and no call between them.
//!
//! * `ITEMS`: `itemSize` is written only with Numbers, among them integer
//!   results (`|0`, lengths, `Math.floor`), -0, NaN and i32 bounds. The
//!   method body multiplies the loaded double with nothing in between, and
//!   the program prints node's output.
//! * `MIXED`: the same class shape, but one write stores a BigInt. F2 holds
//!   `bigint`, so the read stays generic and the BigInt multiply still works
//!   (red if the read is a Number without the kind test).

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

/// Compile `source` with the default (whole-program) facts; return the saved
/// LLVM IR of the entry module and the program's stdout.
fn compile_and_run(dir: &Path, source: &str) -> (String, String) {
    let src = dir.join("main.ts");
    std::fs::write(&src, source).unwrap();
    let exe = dir.join("main_bin");
    let ll_dir = dir.join("ll");
    std::fs::create_dir_all(&ll_dir).unwrap();
    let mut cmd = Command::new(perry_bin());
    let out = clean(&mut cmd)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_SAVE_LL", &ll_dir)
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ll = std::fs::read_to_string(ll_dir.join("main_ts.ll")).expect("saved main_ts.ll");
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed: {:?}", ran.status);
    (ll, String::from_utf8_lossy(&ran.stdout).into_owned())
}

/// The bodies of every function whose `define` line names `name` (the method
/// and its specializations).
fn bodies<'a>(ir: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    for (i, _) in ir.match_indices("define ") {
        let line = ir[i..].lines().next().unwrap_or("");
        if line.contains(name) {
            let end = ir[i..].find("\n}\n").map_or(ir.len(), |e| i + e);
            out.push(&ir[i..end]);
        }
    }
    assert!(!out.is_empty(), "no function {name}");
    out
}

/// Some `fmul double` in `body` has an operand defined by a `load double`
/// with no `icmp`/`select`/`br`/`phi`/`call` between the load and the fmul.
fn fmul_of_plain_load(body: &str) -> bool {
    let lines: Vec<&str> = body.lines().map(str::trim).collect();
    for (m, line) in lines.iter().enumerate() {
        let Some(rest) = line.split_once("= fmul double ").map(|(_, r)| r) else {
            continue;
        };
        for operand in rest
            .split(',')
            .map(|o| o.trim().split_whitespace().next().unwrap_or(""))
        {
            if !operand.starts_with('%') {
                continue;
            }
            let def = format!("{operand} = load double");
            let Some(l) = lines[..m].iter().rposition(|x| x.starts_with(&def)) else {
                continue;
            };
            let clean = lines[l + 1..m].iter().all(|x| {
                !(x.contains(" icmp ")
                    || x.contains(" select ")
                    || x.starts_with("br ")
                    || x.contains(" phi ")
                    || x.contains("call ")
                    || x.ends_with(':'))
            });
            if clean {
                return true;
            }
        }
    }
    false
}

const ITEMS: &str = r#"
class Attr {
  array: Float32Array; itemSize; version;
  constructor(array: Float32Array, itemSize) {
    this.array = array;
    this.itemSize = itemSize;
    this.version = 0;
  }
  scaled(n: number): number { let acc = 0; for (let i = 0; i < 3; i++) acc = i * this.itemSize; return acc; }
}
const data = new Float32Array(8);
const ints = [4, 5, 6];
let k = 0;
for (let i = 0; i < 10; i++) k = (k + 3) | 0;
const sources = [3, 2.5, -0, NaN, ints.length, k, (k * 1000000) | 0, Math.floor(7.9), "ab".length, -2147483648];
const out: string[] = [];
for (const s of sources) {
  const a = new Attr(data, s);
  const r = a.scaled(0);
  out.push(String(r) + (Object.is(r, -0) ? "(-0)" : ""));
}
console.log(out.join(","));
"#;

const MIXED: &str = r#"
class Attr {
  array: Float32Array; itemSize; version;
  constructor(array: Float32Array, itemSize) {
    this.array = array;
    this.itemSize = itemSize;
    this.version = 0;
  }
  scaled(index: number): number { return index * this.itemSize; }
  big(index: bigint) { return index * this.itemSize; }
}
const data = new Float32Array(8);
const a = new Attr(data, 3);
const b = new Attr(data, (globalThis as any).process ? 4n : 4);
let r: string;
try { r = String(b.scaled(2)); } catch (e) { r = (e as Error).constructor.name; }
console.log(String(a.scaled(2)) + "," + String(b.big(5n)) + "," + r);
"#;

#[test]
fn a_field_proven_number_multiplies_without_the_dynamic_helper() {
    let dir = tempfile::tempdir().unwrap();
    let (ir, out) = compile_and_run(dir.path(), ITEMS);
    // The closure wrappers hold no multiply; the method bodies must hold
    // the plain one and never the dynamic helper.
    let all = bodies(&ir, "__Attr__scaled");
    for body in &all {
        assert!(
            !body.contains("js_dynamic_mul"),
            "scaled keeps the dynamic multiply:\n{body}"
        );
    }
    assert!(
        all.iter().any(|b| b.contains("fmul double")),
        "no scaled body has an fmul"
    );
    // The proven read is a plain load used by the fmul: between the load and
    // the fmul there is no icmp, select, br, phi or call. Red on main (the
    // field read keeps its guard and the dynamic multiply).
    assert!(
        all.iter().any(|b| fmul_of_plain_load(b)),
        "no scaled body multiplies a plain field load:\n{}",
        all.join("\n----\n")
    );
    // node: 6,5,0(-0),NaN,6,60,60000000,14,4,-4294967296
    assert_eq!(
        out.trim(),
        "6,5,0(-0),NaN,6,60,60000000,14,4,-4294967296",
        "node's output"
    );
}

#[test]
fn a_field_that_may_hold_a_bigint_stays_generic() {
    let dir = tempfile::tempdir().unwrap();
    let (ir, out) = compile_and_run(dir.path(), MIXED);
    assert!(
        bodies(&ir, "__Attr__scaled")
            .iter()
            .any(|b| b.contains("js_dynamic_mul")),
        "a BigInt-holding field lost its dynamic multiply"
    );
    assert_eq!(out.trim(), "6,20,TypeError", "node's output");
}
