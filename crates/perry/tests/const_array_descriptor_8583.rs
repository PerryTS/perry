//! #8583 follow-up — large constant array literals materialize from a static
//! rodata descriptor + ONE bulk call, and the result is GC-correct, fresh, and
//! byte-identical to the procedural construction path.
//!
//! A minified bundle data table is a giant nested constant array literal. The
//! default lowering builds it with N per-subarray `js_array_from_values`
//! allocations and a huge procedural body (the `__33499` fan-out). The
//! descriptor path serializes the constant tree into a rodata blob and calls
//! `js_value_from_const_descriptor` once to materialize a FRESH, mutable array.
//!
//! Two checks, no node oracle:
//!   * the optimization actually fired — the emitted IR calls
//!     `js_value_from_const_descriptor` and does NOT build the table with a
//!     per-subarray `js_array_from_values` (so this test can't silently become
//!     a tautology if the path stops matching);
//!   * `PERRY_CONST_ARRAY_DESCRIPTOR=1` (default) vs `=0` (procedural) produce
//!     byte-identical output under every moving-collector arm — a mis-rooted
//!     value in the suppressed-GC bulk build would diverge (or crash) in the
//!     descriptor arm only. Mutation-after-materialization is exercised so a
//!     wrongly-shared constant would surface as cross-instance aliasing.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// A large, fully-constant nested array (300 numeric rows > the 256-node gate),
/// one boolean/null row to cover non-numeric constants, then: a fresh second
/// materialization, a mutation of the first (freshness/mutability), and a
/// deterministic checksum. Output is hand-verifiable and identical whichever
/// construction path built the table.
fn source() -> String {
    let mut rows = String::new();
    for i in 0..300 {
        if i > 0 {
            rows.push(',');
        }
        rows.push_str(&format!("[{},{},{}]", i % 128, (i * 7) % 128, (i * 13) % 128));
    }
    // One non-numeric row so bool/null tags are exercised in the descriptor.
    rows.push_str(",[true,null,false]");
    format!(
        r#"
function table() {{ return [{rows}]; }}
const t = table();
const t2 = table();
t[0].push(999);
let sum = 0;
for (let i = 0; i < 300; i++) {{ sum = (sum + t[i][0] + t[i][1] + t[i][2]) | 0; }}
const row = t[300];
console.log(
  "rows:" + t.length +
  " s:" + sum +
  " mut:" + t[0].length +
  " fresh:" + t2[0].length +
  " id:" + (t === t2) +
  " b:" + row[0] + " n:" + row[1] + " f:" + row[2]
);
"#
    )
}

const GC_ENV_OVERRIDES: &[&str] = &[
    "PERRY_GEN_GC",
    "PERRY_GC_SCAVENGE",
    "PERRY_GC_SCAVENGE_NURSERY_MB",
    "PERRY_GC_MOVING_SAFEPOINT",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_CONSERVATIVE_STACK_SCAN",
    "PERRY_WRITE_BARRIERS",
    "PERRY_GC_INCREMENTAL",
    "PERRY_GC_HEAP_LIMIT",
    "PERRY_CONST_ARRAY_DESCRIPTOR",
];

fn compile(dir: &std::path::Path, descriptor: bool) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join(format!("bin_desc_{descriptor}"));
    std::fs::write(&entry, source()).expect("write entry");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for key in GC_ENV_OVERRIDES {
        cmd.env_remove(key);
    }
    if !descriptor {
        cmd.env("PERRY_CONST_ARRAY_DESCRIPTOR", "0");
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "perry compile (descriptor={descriptor}) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    output
}

fn run_arms(binary: &std::path::Path, dir: &std::path::Path, label: &str) -> String {
    let mut arms: Vec<Vec<(&str, &str)>> = vec![vec![]];
    for mb in ["1", "2", "4"] {
        arms.push(vec![("PERRY_GC_SCAVENGE_NURSERY_MB", mb)]);
    }
    arms.push(vec![("PERRY_GEN_GC", "0")]);

    let mut first: Option<String> = None;
    for arm in &arms {
        let mut cmd = Command::new(binary);
        cmd.current_dir(dir);
        for key in GC_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        for (k, v) in arm {
            cmd.env(k, v);
        }
        let run = cmd.output().expect("run compiled binary");
        let arm_label = if arm.is_empty() {
            format!("{label}/default")
        } else {
            format!("{label}/{}={}", arm[0].0, arm[0].1)
        };
        assert!(
            run.status.success(),
            "[{arm_label}] compiled binary failed (exit {:?})\nstderr:\n{}",
            run.status.code(),
            String::from_utf8_lossy(&run.stderr),
        );
        let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
        match &first {
            None => first = Some(stdout),
            Some(f) => assert_eq!(
                &stdout, f,
                "[{arm_label}] output differs between collector arms — a value \
                 materialized in the suppressed-GC bulk build was mis-rooted"
            ),
        }
    }
    first.expect("at least one arm ran")
}

#[test]
fn const_array_descriptor_fires_in_ir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let ll_dir = dir.path().join("ll");
    std::fs::create_dir_all(&ll_dir).unwrap();
    std::fs::write(&entry, source()).expect("write entry");
    let out = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.path().join("unused"))
        .arg("--no-cache")
        .arg("--no-link")
        .env("PERRY_SAVE_LL", &ll_dir)
        .env_remove("PERRY_CONST_ARRAY_DESCRIPTOR")
        .output()
        .expect("run perry compile --no-link");
    assert!(out.status.success(), "compile failed: {}", String::from_utf8_lossy(&out.stderr));

    let ir: String = std::fs::read_dir(&ll_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("ll"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert!(
        ir.contains("js_value_from_const_descriptor"),
        "the large constant table should materialize via js_value_from_const_descriptor"
    );
    // The descriptor path must replace the per-subarray builder for this table:
    // no `js_array_from_values` CALL should remain (the always-present `declare`
    // line is filtered out).
    assert!(
        !ir.contains("call i64 @js_array_from_values("),
        "no per-subarray js_array_from_values call should remain for the const table"
    );
}

#[test]
fn const_array_descriptor_matches_procedural_under_moving_gc() {
    let dir = tempfile::tempdir().expect("tempdir");
    let descriptor_bin = compile(dir.path(), true);
    let procedural_bin = compile(dir.path(), false);

    let descriptor_out = run_arms(&descriptor_bin, dir.path(), "descriptor");
    let procedural_out = run_arms(&procedural_bin, dir.path(), "procedural");

    // Structural correctness (robust to the exact checksum, which the
    // differential below pins anyway): 301 rows; the two materializations are
    // DISTINCT instances (id:false); mutating t[0] (3 -> 4 after push) did not
    // touch the fresh t2[0] (still 3) — proving each call yields a fresh mutable
    // array, not a shared constant; and the non-numeric row round-trips.
    assert!(
        descriptor_out.starts_with("rows:301 s:54810 mut:4 fresh:3 id:false"),
        "unexpected descriptor output: {descriptor_out:?}"
    );
    assert!(
        descriptor_out.trim_end().ends_with("b:true n:null f:false"),
        "non-numeric constants must round-trip: {descriptor_out:?}"
    );
    assert_eq!(
        descriptor_out, procedural_out,
        "descriptor materialization diverged from the procedural construction path"
    );
}
