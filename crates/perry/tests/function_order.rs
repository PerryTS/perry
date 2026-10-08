//! `--record-function-order` / `--function-order`: a recording build writes
//! the order its functions first ran, and a build given that list places
//! those functions first, in list order. A normal build carries no recording
//! hook, and a stale list is harmless.

use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = r#"
function delta(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += i * 3; return s + n; }
function gamma(n: number): number { let s = 1; for (let i = 0; i < n; i++) s = (s * 7 + i) % 1000; return s; }
function beta(n: number): number { let s = n; for (let i = 0; i < 5; i++) s = (s ^ (i * 13)) + 1; return s; }
function alpha(n: number): number { let s = 0; for (let i = 0; i < n % 17; i++) s += i; return s - 1; }
function neverRun(n: number): number { let s = 0; for (let i = 0; i < n; i++) s += i * i; return s; }
const steps = [delta, gamma, beta, alpha];
let x = Number(process.argv.length) + 20;
for (const step of steps) x = step(x);
if (x === -12345) x = neverRun(x);
console.log("result " + x);
"#;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, out: &str, extra: &[&str]) -> PathBuf {
    let output = dir.join(out);
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .env("PERRY_KEEP_SYMBOLS", "1")
        .arg("compile")
        .arg(dir.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .args(extra)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile {extra:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(binary: &Path, record_to: Option<&Path>) -> String {
    let mut cmd = Command::new(binary);
    if let Some(path) = record_to {
        cmd.env("PERRY_FUNCTION_ORDER_OUT", path);
    }
    let run = cmd.output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "binary failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Defined text symbols in address order.
fn text_symbols(binary: &Path) -> Vec<String> {
    let nm = Command::new("nm")
        .arg("-n")
        .arg("--defined-only")
        .arg(binary)
        .output()
        .expect("run nm");
    assert!(nm.status.success());
    String::from_utf8_lossy(&nm.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (_addr, kind, name) = (parts.next()?, parts.next()?, parts.next()?);
            matches!(kind, "t" | "T").then(|| name.to_string())
        })
        .collect()
}

fn user_symbol(names: &[String], function: &str) -> String {
    // The function body itself, not its `__perry_wrap_*` callable wrapper.
    let suffix = format!("__{function}");
    let matches: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("perry_fn_") && n.ends_with(&suffix))
        .collect();
    assert_eq!(matches.len(), 1, "one symbol for {function}: {matches:?}");
    matches[0].clone()
}

fn position(symbols: &[String], name: &str) -> usize {
    symbols
        .iter()
        .position(|s| s == name)
        .unwrap_or_else(|| panic!("{name} is not a text symbol of the binary"))
}

/// Record a run; returns the recorded names.
fn record(dir: &Path) -> (Vec<String>, String) {
    let recorder = compile(dir, "recorder", &["--record-function-order"]);
    let list = dir.join("recorded.txt");
    let stdout = run(&recorder, Some(&list));
    let names: Vec<String> = std::fs::read_to_string(&list)
        .expect("the recording binary wrote its order")
        .lines()
        .map(str::to_string)
        .collect();
    (names, stdout)
}

#[cfg_attr(not(target_os = "linux"), ignore = "checks ELF symbol order with nm")]
#[test]
fn a_recorded_order_is_the_layout_and_a_stale_list_is_harmless() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.ts"), SOURCE).unwrap();

    let (recorded, recorded_stdout) = record(dir.path());
    let mut unique = recorded.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        recorded.len(),
        "each function is recorded once"
    );
    assert!(recorded.iter().any(|n| n == "main"), "{recorded:?}");
    let [delta, gamma, beta, alpha] =
        ["delta", "gamma", "beta", "alpha"].map(|f| user_symbol(&recorded, f));
    let at = |name: &String| recorded.iter().position(|n| n == name).unwrap();
    assert!(
        at(&delta) < at(&gamma) && at(&gamma) < at(&beta) && at(&beta) < at(&alpha),
        "first-execution order: {recorded:?}"
    );
    assert!(
        !recorded.iter().any(|n| n.ends_with("neverRun")),
        "a function that never ran is not recorded: {recorded:?}"
    );

    // A normal build carries no hook and writes nothing.
    let plain = compile(dir.path(), "plain", &[]);
    let untouched = dir.path().join("untouched.txt");
    let plain_stdout = run(&plain, Some(&untouched));
    assert_eq!(plain_stdout, recorded_stdout);
    assert!(
        !untouched.exists(),
        "a normal build must not record its function order"
    );
    let plain_symbols = text_symbols(&plain);
    assert!(
        !plain_symbols
            .iter()
            .any(|s| s.contains("js_function_order_first_call")),
        "a normal build must not link the recording hook"
    );
    let never_run = user_symbol(&plain_symbols, "neverRun");

    // The four user functions in REVERSE execution order, then everything
    // else that ran: the layout must follow the list, not the run.
    let mut list = vec![alpha.clone(), beta.clone(), gamma.clone(), delta.clone()];
    let rest: Vec<String> = recorded
        .iter()
        .filter(|n| !list.contains(n))
        .cloned()
        .collect();
    list.extend(rest);
    std::fs::write(dir.path().join("order.txt"), list.join("\n")).unwrap();
    let ordered = compile(dir.path(), "ordered", &["--function-order", "order.txt"]);
    assert_eq!(run(&ordered, None), recorded_stdout);
    let symbols = text_symbols(&ordered);
    let placed: Vec<usize> = [&alpha, &beta, &gamma, &delta]
        .iter()
        .map(|n| position(&symbols, n))
        .collect();
    assert!(
        placed.windows(2).all(|w| w[0] < w[1]),
        "listed functions follow the list: {placed:?}"
    );
    assert!(
        placed[3] < position(&symbols, &never_run),
        "listed functions precede unlisted generated code"
    );

    // A stale, partial list: unknown names, and only two known ones.
    std::fs::write(
        dir.path().join("stale.txt"),
        format!("# stale\nno_such_function_1\n{gamma}\nperry_fn_gone__x\n{beta}\n"),
    )
    .unwrap();
    let stale = compile(dir.path(), "stale", &["--function-order", "stale.txt"]);
    assert_eq!(run(&stale, None), recorded_stdout);
    let symbols = text_symbols(&stale);
    assert!(position(&symbols, &gamma) < position(&symbols, &beta));
    assert!(position(&symbols, &beta) < position(&symbols, &delta));
}

#[test]
fn the_two_flags_are_exclusive_and_a_missing_list_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.ts"), "console.log(1);\n").unwrap();
    let both = Command::new(perry_bin())
        .current_dir(dir.path())
        .args([
            "compile",
            "main.ts",
            "--record-function-order",
            "--function-order",
            "x.txt",
        ])
        .output()
        .unwrap();
    assert!(!both.status.success());
    let missing = Command::new(perry_bin())
        .current_dir(dir.path())
        .args([
            "compile",
            "main.ts",
            "--no-cache",
            "--function-order",
            "nope.txt",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("--function-order"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}
