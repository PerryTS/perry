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
        .arg("--no-auto-optimize")
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
    let runtime = plain_symbols
        .iter()
        .find(|n| n.as_str() == "js_string_concat")
        .expect("runtime string concat")
        .clone();
    let rust = plain_symbols
        .iter()
        .find(|n| n.starts_with("_R") && n.contains("perry_runtime"))
        .expect("a Rust runtime symbol")
        .clone();
    let mut list = vec![
        runtime.clone(),
        rust.clone(),
        alpha.clone(),
        beta.clone(),
        gamma.clone(),
        delta.clone(),
    ];
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
    assert!(position(&symbols, &runtime) < position(&symbols, &rust));
    assert!(position(&symbols, &rust) < position(&symbols, &alpha));
    let repeated = compile(dir.path(), "repeated", &["--function-order", "order.txt"]);
    assert_eq!(
        symbols,
        text_symbols(&repeated),
        "deterministic symbol layout"
    );
    assert_eq!(
        std::fs::read(&ordered).unwrap(),
        std::fs::read(&repeated).unwrap(),
        "deterministic link bytes",
    );
    // Same named code as the ordinary link: layout cannot introduce clones.
    let mut plain_set = plain_symbols.clone();
    let mut ordered_set = symbols.clone();
    plain_set.sort();
    ordered_set.retain(|n| !n.contains("hot_text") && !n.contains("thp_enabled"));
    ordered_set.sort();
    // Startup support is allowed to pull in libc/Rust helpers. All original
    // generated and runtime symbols retain their original multiplicity. Rust
    // static archives can already contain repeated local/weak symbol names.
    for n in &plain_set {
        assert_eq!(
            ordered_set.iter().filter(|m| *m == n).count(),
            plain_set.iter().filter(|m| *m == n).count(),
            "layout must preserve the existing symbol multiplicity: {n}",
        );
    }

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

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn backing_bounds_cover_complete_selected_pages_without_text_padding() {
    use perry_codegen::FunctionOrder;
    const HP: u64 = 2 * 1024 * 1024;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("order.ld");
    std::fs::write(
        &script,
        FunctionOrder::parse("sampled").elf_linker_script().unwrap(),
    )
    .unwrap();
    for bytes in [HP - 32, HP, HP + 32] {
        let source = dir.path().join("fixture.s");
        // A sized selected body followed immediately by ordinary code. The
        // body is not executed; this checks the linker's actual boundaries.
        std::fs::write(&source, format!(
            ".section .text.sampled,\"ax\",@progbits\n.balign 16\n.globl sampled\n.type sampled,@function\nsampled:\n.fill {},1,0x90\nret\n.size sampled,.-sampled\n.text\n.globl main\n.type main,@function\nmain:\nxor %eax,%eax\nret\n.size main,.-main\n.section .note.GNU-stack,\"\",@progbits\n", bytes - 1
        )).unwrap();
        let binary = dir.path().join("fixture");
        let linked = Command::new("cc")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .arg(format!("-Wl,-T,{}", script.display()))
            .arg("-Wl,-z,separate-code")
            .output()
            .unwrap();
        assert!(
            linked.status.success(),
            "{}",
            String::from_utf8_lossy(&linked.stderr)
        );
        let nm = Command::new("nm")
            .arg("--defined-only")
            .arg(&binary)
            .output()
            .unwrap();
        let names: std::collections::HashMap<String, u64> = String::from_utf8_lossy(&nm.stdout)
            .lines()
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                (fields.len() == 3).then(|| {
                    (
                        fields[2].to_string(),
                        u64::from_str_radix(fields[0], 16).unwrap(),
                    )
                })
            })
            .collect();
        let start = names["__perry_hot_text_start"];
        let end = names["__perry_hot_text_end"];
        let limit = names["__perry_hot_text_limit"];
        assert_eq!(start % 16, 0);
        assert_eq!(end - start, bytes);
        assert_eq!(limit, end);
        assert!(limit <= end, "backing must not include unselected code");
        assert_eq!(names["_start"], end, "no padding after the selected body");
        let headers = Command::new("readelf")
            .args(["-W", "-l"])
            .arg(&binary)
            .output()
            .unwrap();
        let headers = String::from_utf8_lossy(&headers.stdout);
        let loads: Vec<_> = headers
            .lines()
            .filter(|line| line.trim_start().starts_with("LOAD "))
            .collect();
        assert!(
            loads.iter().all(|line| line.trim_end().ends_with("0x1000")),
            "{headers}"
        );
        assert!(
            loads
                .iter()
                .filter(|line| !line.contains("R E"))
                .all(|line| line.trim_end().ends_with("0x1000")),
            "{headers}"
        );
    }
}
