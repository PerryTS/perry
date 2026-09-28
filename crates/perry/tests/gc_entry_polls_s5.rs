//! RFC deferred collection S5, decision 2: function-entry GC polls.
//!
//! Loop back-edge polls bound allocation in iteration; these tests pin the two
//! other placements — one poll per recursive SCC of the direct call graph, and
//! an indirect-entry poll in closures, methods and value wrappers — and the
//! placement they must NOT have: a non-recursive, directly-called function gets
//! none (the census's reason for rejecting polls at every function entry).
//!
//! The runtime half asserts its subject was live: a loop-free recursive
//! allocator reaches entry polls, drains its nursery at them, and never needs
//! the allocation-point valve.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const ENTRY_POLL: &str = "call void @js_gc_entry_safepoint()";
const WRAPPER_POLL: &str = "call void @js_gc_entry_safepoint_args(";

fn compile(dir: &Path, source: &str, env: &[(&str, &str)]) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let mut command = Command::new(perry_bin());
    command
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1")
        .env_remove("PERRY_GC_MOVING_LOOP_POLLS");
    for (key, value) in env {
        command.env(key, value);
    }
    let compile = command.output().expect("run perry compile");
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

fn kept_ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
}

/// `(name, body)` for every `define` in `ir`.
fn functions(ir: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in ir.lines() {
        if line.starts_with("define ") {
            let name = line
                .split('@')
                .nth(1)
                .and_then(|rest| rest.split('(').next())
                .unwrap_or("")
                .trim_matches('"')
                .to_string();
            current = Some((name, String::new()));
        } else if line == "}" {
            if let Some(done) = current.take() {
                out.push(done);
            }
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

/// Entry polls in every non-wrapper clone of the function named `suffix`
/// (codegen emits `$spec_*`, `.__arena` and `$generic` clones; each clone
/// family can form its own recursive SCC), keyed by the clone's name.
fn polls_by_clone(functions: &[(String, String)], suffix: &str) -> Vec<(String, usize)> {
    let tag = format!("__{suffix}");
    let clones: Vec<(String, usize)> = functions
        .iter()
        .filter(|(name, _)| {
            !name.starts_with("__perry_wrap_")
                && name.split_once(&tag).is_some_and(|(_, rest)| {
                    rest.is_empty() || !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                })
        })
        .map(|(name, body)| (name.clone(), body.matches(ENTRY_POLL).count()))
        .collect();
    assert!(!clones.is_empty(), "no function `{suffix}` in the IR");
    clones
}

fn total_polls(functions: &[(String, String)], suffix: &str) -> usize {
    polls_by_clone(functions, suffix)
        .iter()
        .map(|(_, n)| n)
        .sum()
}

fn wrapper_of<'a>(functions: &'a [(String, String)], suffix: &str) -> &'a str {
    functions
        .iter()
        .find(|(name, _)| name.starts_with("__perry_wrap_") && name.ends_with(suffix))
        .map(|(_, body)| body.as_str())
        .unwrap_or_else(|| panic!("no value wrapper ending `{suffix}`"))
}

const PLACEMENT_SOURCE: &str = r#"
function tree(d: number): any {
  return d > 0 ? { l: tree(d - 1), r: tree(d - 1) } : null;
}
function isEven(n: number): any {
  return n === 0 ? [n] : isOdd(n - 1);
}
function isOdd(n: number): any {
  return n === 0 ? [n, n] : isEven(n - 1);
}
function mk(n: number): any {
  return { n: n };
}
function pure(n: number): number {
  return n * 2 + 1;
}
class Box {
  v: number;
  constructor(v: number) { this.v = v; }
  wrap(x: number): any { return [this.v, x]; }
  plain(x: number): number { return this.v + x; }
}
const toObj = (x: number) => ({ x: x });
const b = new Box(3);
const mapped = [1, 2, 3].map(toObj).map((o: any) => mk(o.x)).map((o: any) => pure(o.n));
const counted = [4, 5].map(pure);
console.log(JSON.stringify(tree(2)) !== "", isEven(10).length, mapped.join(","),
  counted.join(","), b.wrap(1).length, b.plain(2));
"#;

#[test]
fn entry_polls_go_to_recursive_sccs_and_indirect_entries_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), PLACEMENT_SOURCE, &[]);
    let ir = kept_ir(&stderr);
    let fns = functions(&ir);

    // Recursion: a self-recursive allocator polls at its entry ...
    assert!(
        total_polls(&fns, "tree") >= 1,
        "a recursive SCC of one member must carry the entry poll: {:?}",
        polls_by_clone(&fns, "tree")
    );
    // ... a mutually recursive pair carries exactly ONE poll per SCC: each
    // clone family (`$spec_*`, `$generic`, the boxed body) pairs isEven with
    // isOdd of the same family, so no family may poll in both.
    let even = polls_by_clone(&fns, "isEven");
    let odd = polls_by_clone(&fns, "isOdd");
    assert!(
        even.iter().chain(&odd).map(|(_, n)| n).sum::<usize>() >= 1,
        "the mutually recursive pair must poll: {even:?} {odd:?}"
    );
    for (name, polls) in &even {
        let partner = name.replace("__isEven", "__isOdd");
        let partner_polls = odd
            .iter()
            .find(|(n, _)| *n == partner)
            .map_or(0, |(_, p)| *p);
        assert!(
            polls + partner_polls <= 1,
            "one poll per recursive SCC: {name}={polls}, {partner}={partner_polls}"
        );
    }
    // A non-recursive function entered directly gets none, allocating or not.
    assert_eq!(
        total_polls(&fns, "mk"),
        0,
        "a non-recursive direct-call function must not poll at entry"
    );
    assert_eq!(total_polls(&fns, "pure"), 0);

    // Indirect entry: an allocating method polls, a non-allocating one does not.
    assert!(
        total_polls(&fns, "wrap") >= 1,
        "an allocating method is entered indirectly and must poll"
    );
    assert_eq!(total_polls(&fns, "plain"), 0);
    // An allocating closure body polls.
    assert!(
        fns.iter()
            .any(|(name, body)| name.starts_with("perry_closure_") && body.contains(ENTRY_POLL)),
        "the allocating arrow's body must poll at entry"
    );
    // The value wrapper that makes `mk` a callback polls (with its args
    // spilled for the runtime to root); `pure`'s wrapper forwards to a proven
    // leaf and does not.
    assert!(wrapper_of(&fns, "__mk").contains(WRAPPER_POLL));
    assert!(!wrapper_of(&fns, "__pure").contains(WRAPPER_POLL));

    let run = Command::new(&bin).output().expect("run");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "true 1 3,5,7 9,11 2 5"
    );
}

/// The kill switch removes the entry polls with the loop polls: one switch,
/// one decision (`PERRY_GC_MOVING_LOOP_POLLS=0`).
#[test]
fn the_loop_poll_kill_switch_removes_entry_polls_too() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_bin, stderr) = compile(
        dir.path(),
        PLACEMENT_SOURCE,
        &[("PERRY_GC_MOVING_LOOP_POLLS", "0")],
    );
    let ir = kept_ir(&stderr);
    assert!(!ir.contains(ENTRY_POLL));
    assert!(!ir.contains(WRAPPER_POLL));
}

fn diag_field(stderr: &str, line_prefix: &str, field: &str) -> u64 {
    let line = stderr
        .lines()
        .find(|line| line.starts_with(line_prefix))
        .unwrap_or_else(|| panic!("no `{line_prefix}` line in:\n{stderr}"));
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{field}=")))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("no numeric `{field}` in `{line}`"))
}

fn run_with_diag(bin: &Path) -> Output {
    let mut command = Command::new(bin);
    for key in [
        "PERRY_GC_SCHEDULE_SEED",
        "PERRY_GC_FORCE_EVACUATE",
        "PERRY_GC_MOVING_LOOP_POLLS",
        "PERRY_GC_VALVE_LEDGER",
    ] {
        command.env_remove(key);
    }
    command.env("PERRY_GC_DIAG", "1").output().expect("run")
}

/// A loop-free recursive allocator: ~8 M calls, ~0.5 GB of garbage, a live set
/// of one recursion path. Without a poll inside the recursion the only thing
/// that could collect is the allocation-point valve.
const RECURSIVE_CHURN: &str = r#"
function churn(d: number): number {
  if (d === 0) return 1;
  const junk = { a: d, b: [d, d, d] };
  return churn(d - 1) + churn(d - 1) + (junk.b.length - 3);
}
console.log(churn(22));
"#;

#[test]
fn a_loop_free_recursion_drains_at_its_scc_poll_and_never_needs_the_valve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _stderr) = compile(dir.path(), RECURSIVE_CHURN, &[]);
    let run = run_with_diag(&bin);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "4194304");

    let entry_polls = diag_field(&stderr, "[gc-alloc-point]", "entry_polls");
    let drains = diag_field(&stderr, "[gc-alloc-point]", "safepoint_drains");
    let valve = diag_field(&stderr, "[gc-alloc-point]", "valve_fires");
    let wait = diag_field(&stderr, "[gc-alloc-point]", "max_poll_wait_bytes");
    // Subject live: the SCC poll was reached armed, and collections drained there.
    assert!(
        entry_polls > 0,
        "no armed entry poll was reached:\n{stderr}"
    );
    assert!(drains > 0, "no collection drained at a poll:\n{stderr}");
    // ... so the valve never had to fire, and a deferred collection waited far
    // less than the valve's 64 MiB slack for its poll.
    assert_eq!(valve, 0, "{stderr}");
    assert!(
        wait < 16 * 1024 * 1024,
        "a deferred collection waited {wait} bytes for a poll"
    );
}
