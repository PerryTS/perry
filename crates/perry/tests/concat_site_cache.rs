//! Per-site concat cache for `"literal" + value`
//! (`perry-codegen/src/concat_site_cache.rs`,
//! `perry-runtime/src/string/concat_site.rs`).
//!
//! Node is the oracle for every value below; a hand-computed expectation has
//! been wrong before while perry was right. The IR pins say the lane FIRES
//! (a program can be correct because the lane never ran), and the kill switch
//! proves the plain fused helper still answers the same program.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// The bench_object_property key shape, then every edge of the slot rule:
/// a proven bound past the table (32..39 take the plain arm through the
/// lane), a `+=` on a handle the cache handed out, `-0`, fractional /
/// negative / NaN / huge right operands, dynamic operands of number, string
/// and null type at a literal-prefix site, and the SSO twin.
const SOURCE: &str = r#"
const OBJECTS = 200;
const FIELDS = 20;
let checksum = 0;
for (let i = 0; i < OBJECTS; i++) {
  const obj: any = {};
  for (let j = 0; j < FIELDS; j++) {
    obj["field_" + j] = i * FIELDS + j;
  }
  checksum += obj["field_0"] + obj["field_" + (FIELDS - 1)];
}
console.log("checksum:" + checksum);

const parts: string[] = [];
for (let j = 0; j < 40; j++) {
  parts.push("field_" + j);
}
let s = "field_" + 3;
s += "!";
parts.push(s);
parts.push("field_" + 3);
parts.push("field_" + (-0));
parts.push("field_" + 1.5);
parts.push("field_" + (-1));
parts.push("field_" + NaN);
parts.push("field_" + 1e21);
let dyn: any = 7;
parts.push("field_" + dyn);
dyn = "x";
parts.push("field_" + dyn);
dyn = null;
parts.push("field_" + dyn);
parts.push("k" + 4);
console.log(parts.join(","));
"#;

fn compile(dir: &Path, source: &str, extra_env: &[(&str, &str)]) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let compile = cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    (
        output,
        String::from_utf8_lossy(&compile.stderr).into_owned(),
    )
}

/// CALL sites of `name`, not the `declare` line every module emits for every
/// runtime symbol (a bare-substring presence test passes vacuously and a
/// bare-substring absence test can never pass).
fn call_count(ir: &str, name: &str) -> usize {
    let needle = format!("@{name}(");
    ir.lines()
        .filter(|l| l.contains(&needle) && l.contains("call "))
        .count()
}

fn kept_ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
}

fn node_oracle(dir: &Path) -> String {
    let node = Command::new("node")
        .current_dir(dir)
        .arg("--experimental-strip-types")
        .arg(dir.join("main.ts"))
        .output()
        .expect("run node");
    assert!(
        node.status.success(),
        "node failed on the oracle fixture\n{}",
        String::from_utf8_lossy(&node.stderr)
    );
    String::from_utf8_lossy(&node.stdout).into_owned()
}

fn run(bin: &Path, dir: &Path, gc_stress: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if gc_stress {
        command
            .env("PERRY_GC_HEAP_LIMIT", "8")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    }
    command.output().expect("run compiled binary")
}

fn assert_matches_node(bin: &Path, dir: &Path, expected: &str, label: &str) {
    for stress in [false, true] {
        let out = run(bin, dir, stress);
        assert!(
            out.status.success(),
            "{label}: binary failed (gc_stress={stress})\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            expected,
            "{label}: output differs from node (gc_stress={stress})"
        );
    }
}

/// The lane fires on every literal-prefix site, and the program is
/// node-exact — including under forced, verified evacuation, which is the
/// arm that matters for a cache whose entries are roots that must be
/// rewritten when the cached string moves.
#[test]
fn site_cache_fires_and_matches_node_under_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE, &[]);
    let ir = kept_ir(&stderr);
    assert!(
        call_count(&ir, "js_string_concat_site_value") > 0,
        "the per-site lane's miss arm must be CALLED — the lane did not fire"
    );
    assert!(
        ir.contains("@perry_concat_site_"),
        "the emitted probe must read a per-site table"
    );
    let expected = node_oracle(dir.path());
    assert_matches_node(&bin, dir.path(), &expected, "site-cache");
}

/// Admission follows the proven bound: a counter that sweeps to 100k gets no
/// table (its gate would be pure cost, ~1-2 ns per call, and bench_gc_pressure
/// measured exactly that), while a counter bounded by a small module constant
/// does.
#[test]
fn admission_follows_the_proven_bound() {
    const LARGE: &str = r#"
let big = 0;
for (let i = 0; i < 100000; i++) {
  big += ("big_" + i).length;
}
console.log("big:" + big);
"#;
    // Three admitted sites: the bounded counter, a constant expression over a
    // module constant, and an integer literal. `"n:" + n` is not one (`n` is
    // an accumulator with no proven interval), so the table count is exact.
    const SMALL: &str = r#"
const FIELDS = 20;
let n = 0;
for (let j = 0; j < FIELDS; j++) {
  n += ("field_" + j).length;
  n += ("field_" + (FIELDS - 1)).length;
  n += ("field_" + 19).length;
}
console.log("n:" + n);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), LARGE, &[]);
    let ir = kept_ir(&stderr);
    assert_eq!(
        call_count(&ir, "js_string_concat_site_value"),
        0,
        "a counter proven to sweep far past the table must not get one"
    );
    assert!(
        !ir.contains("@perry_concat_site_"),
        "no per-site table may be emitted for the large-bound site"
    );
    assert!(
        call_count(&ir, "js_string_concat_value_box") > 0,
        "vacuity guard: the large-bound site must still reach the fused arm"
    );
    let expected = node_oracle(dir.path());
    assert_matches_node(&bin, dir.path(), &expected, "large-bound");

    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SMALL, &[]);
    let ir = kept_ir(&stderr);
    let tables = ir
        .lines()
        .filter(|l| l.contains("@perry_concat_site_") && l.contains("= private global"))
        .count();
    assert_eq!(
        tables, 3,
        "a small-bound counter, a constant expression over a module constant \
         and an integer literal must each get a table (and the unbounded \
         accumulator must not)"
    );
    assert!(
        call_count(&ir, "js_string_concat_site_value") > 0,
        "the admitted sites' fill arm must be CALLED"
    );
    let expected = node_oracle(dir.path());
    assert_matches_node(&bin, dir.path(), &expected, "small-bound");
}

/// Kill switch: `PERRY_CONCAT_SITE_CACHE=0` at build time removes the lane,
/// the same sites go back to the plain fused helper (vacuity guard: the
/// shape must still reach that arm), and the answers are unchanged.
#[test]
fn kill_switch_restores_the_plain_helper_and_stays_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE, &[("PERRY_CONCAT_SITE_CACHE", "0")]);
    let ir = kept_ir(&stderr);
    assert_eq!(
        call_count(&ir, "js_string_concat_site_value"),
        0,
        "kill switch must remove the per-site lane"
    );
    assert!(
        call_count(&ir, "js_string_concat_value_box") > 0,
        "fixture no longer reaches the fused literal-prefix arm — the absence \
         assertion above would pass vacuously"
    );
    let expected = node_oracle(dir.path());
    assert_matches_node(&bin, dir.path(), &expected, "kill-switch");
}
