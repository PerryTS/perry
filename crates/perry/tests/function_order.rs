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
// A value use keeps `neverRun`'s callable wrapper in the binary: generated
// code that never runs and is listed nowhere.
const later = [neverRun];
if (x === -12345) x = later[0](x);
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
        // A `--function-order` link always takes GNU ld (`link/elf_linker.rs`);
        // the plain build it is compared with must use the same linker, as
        // the two keep different sets of duplicate local symbols.
        .env("PERRY_LINK_LLD", "0")
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

/// The symbol of `function`'s code, named as the compiler names it
/// (`perry_codegen::symbol_names`). The function body (`perry_fn_*`) when it
/// is a symbol of its own; a small typed function's public entry is
/// `alwaysinline` (P2 inline admissions, d08e52eaa8), so once every call site
/// has inlined it the body runs inside its callable wrapper
/// (`__perry_wrap_perry_fn_*`, which `steps` calls) and only that is left.
fn user_symbol(names: &[String], function: &str) -> String {
    use perry_codegen::symbol_names;
    let prefix = symbol_names::module_prefix("main.ts");
    let body = symbol_names::function(&prefix, function);
    let wrapper = symbol_names::function_wrapper(&prefix, function);
    let count = |name: &str| names.iter().filter(|n| *n == name).count();
    assert!(
        count(&body) <= 1 && count(&wrapper) <= 1,
        "one symbol each for {function}: {names:?}"
    );
    if count(&body) == 1 {
        body
    } else {
        assert_eq!(
            count(&wrapper),
            1,
            "{function}: neither {body} nor {wrapper} in {names:?}"
        );
        wrapper
    }
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
    // The plain build carries the runtime's default hot-text run and its
    // bound symbols (`link/hot_text_order.rs`); a `--function-order` link
    // replaces that layout, so they are no code of the program's.
    plain_set.retain(|n| !n.starts_with("__perry_rt_hot_text_"));
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

/// Defined symbols by name, with their address.
#[cfg(target_os = "linux")]
fn symbol_addresses(binary: &Path) -> Vec<(u64, String, String)> {
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
            let (addr, kind, name) = (parts.next()?, parts.next()?, parts.next()?);
            Some((
                u64::from_str_radix(addr, 16).ok()?,
                kind.to_string(),
                name.to_string(),
            ))
        })
        .collect()
}

/// The runtime's default hot-text order (`link/hot_text_order.rs`): an
/// ordinary ELF build packs the listed runtime functions at the front of
/// `.text`, the list still names the runtime that is built here, and
/// `PERRY_HOT_TEXT_ORDER=off` restores the unordered layout with the same
/// code.
#[cfg(target_os = "linux")]
#[test]
fn the_default_runtime_order_packs_the_listed_functions_and_the_list_is_current() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.ts"), SOURCE).unwrap();
    let build = |out: &str, switch: Option<&str>| {
        let output = dir.path().join(out);
        let mut cmd = Command::new(perry_bin());
        cmd.current_dir(dir.path())
            .env("PERRY_KEEP_SYMBOLS", "1")
            .env_remove("PERRY_HOT_TEXT_ORDER")
            .arg("compile")
            .arg(dir.path().join("main.ts"))
            .arg("-o")
            .arg(&output)
            .args(["--no-cache", "--no-auto-optimize", "--verbose"]);
        if let Some(value) = switch {
            cmd.env("PERRY_HOT_TEXT_ORDER", value);
        }
        let compile = cmd.output().expect("run perry compile");
        let stderr = String::from_utf8_lossy(&compile.stderr).into_owned();
        assert!(compile.status.success(), "perry compile failed:\n{stderr}");
        (output, stderr)
    };
    let (on, on_log) = build("on", None);
    let (off, off_log) = build("off", Some("off"));
    assert_eq!(run(&on, None), run(&off, None));

    // The list against this build's runtime archives.
    let summary = on_log
        .lines()
        .find(|l| l.starts_with("[hot-text] "))
        .unwrap_or_else(|| panic!("the link reports the default order:\n{on_log}"));
    let field = |name: &str| -> Vec<u64> {
        let at = summary
            .find(name)
            .unwrap_or_else(|| panic!("{name} in {summary}"))
            + name.len();
        summary[at..]
            .split_whitespace()
            .next()
            .unwrap()
            .split('/')
            .map(|n| n.parse().unwrap())
            .collect()
    };
    let (listed, resolved, core) = (field("listed=")[0], field("resolved=")[0], field("core="));
    // A backstop against wholesale rot (a mangling or crate-name change),
    // loose enough for any build profile of the archives; the strict gate is
    // `scripts/hot_text_order.py check`.
    assert!(
        resolved * 100 >= listed * 75 && core[0] * 100 >= core[1] * 90,
        "the default hot-text order no longer names this runtime ({summary}): \
         rebuild it (scripts/hot_text_order.py; `check` lists the missing names)"
    );
    assert!(
        !off_log.contains("[hot-text]"),
        "the switch keeps the JS text rule alone:\n{off_log}"
    );

    // The hot run: bounded, at the front of the text, and it holds code.
    let symbols = symbol_addresses(&on);
    let at = |name: &str| symbols.iter().find(|s| s.2 == name).map(|s| s.0);
    let (start, end) = (
        at("__perry_rt_hot_text_start").expect("hot run start"),
        at("__perry_rt_hot_text_end").expect("hot run end"),
    );
    let text: Vec<&(u64, String, String)> = symbols
        .iter()
        .filter(|s| matches!(s.1.as_str(), "t" | "T") && !s.2.starts_with("__perry_rt_hot_text_"))
        .collect();
    let inside = text.iter().filter(|s| start <= s.0 && s.0 < end).count();
    assert!(inside >= 200, "{inside} functions in the hot run");
    assert!(
        text.iter().filter(|s| s.0 < start).count() < 64,
        "the hot run leads the runtime text"
    );
    // The program's own code is never part of it.
    let generated: Vec<_> = text.iter().filter(|s| s.2.contains("main_ts__")).collect();
    assert!(!generated.is_empty(), "the program's functions are symbols");
    assert!(
        generated.iter().all(|s| s.0 >= end),
        "generated code stays behind the hot run"
    );

    // Off: no run, and the same named code as on.
    let off_symbols = symbol_addresses(&off);
    assert!(off_symbols
        .iter()
        .all(|s| !s.2.starts_with("__perry_rt_hot_text_")));
    let names = |symbols: &[(u64, String, String)]| {
        let mut names: Vec<String> = symbols
            .iter()
            .filter(|s| {
                matches!(s.1.as_str(), "t" | "T") && !s.2.starts_with("__perry_rt_hot_text_")
            })
            .map(|s| s.2.clone())
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&symbols), names(&off_symbols), "layout only");
}

/// A link line that picks lld itself (the cross links to Linux pass
/// `-fuse-ld=lld`, `link/platform_cmd.rs`; here `PERRY_EXTRA_LINK_ARGS` gives
/// the native link the same driver flag) takes the lld flavor of the default
/// hot-text script: lld rejects the GNU one ("unable to insert .text before
/// .text"). The run is in place and the program's output is GNU ld's.
#[cfg(target_os = "linux")]
#[test]
fn a_link_line_that_picks_lld_takes_the_lld_hot_text_script() {
    let lld_on_path = std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("ld.lld").is_file()));
    if !lld_on_path {
        eprintln!("skipped: no ld.lld on PATH");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.ts"), SOURCE).unwrap();
    let build = |out: &str, extra_link_args: Option<&str>| {
        let output = dir.path().join(out);
        let mut cmd = Command::new(perry_bin());
        cmd.current_dir(dir.path())
            .env("PERRY_KEEP_SYMBOLS", "1")
            .env_remove("PERRY_HOT_TEXT_ORDER")
            .env_remove("PERRY_LINK_LLD")
            .env_remove("PERRY_EXTRA_LINK_ARGS")
            .arg("compile")
            .arg(dir.path().join("main.ts"))
            .arg("-o")
            .arg(&output)
            .args(["--no-cache", "--no-auto-optimize", "--verbose"]);
        if let Some(args) = extra_link_args {
            cmd.env("PERRY_EXTRA_LINK_ARGS", args);
        }
        let compile = cmd.output().expect("run perry compile");
        let stderr = String::from_utf8_lossy(&compile.stderr).into_owned();
        assert!(compile.status.success(), "perry compile failed:\n{stderr}");
        (output, stderr)
    };
    let (gnu, gnu_log) = build("gnu", None);
    let (lld, lld_log) = build("lld", Some("-fuse-ld=lld -Wl,--image-base=0x400000"));
    let script = |log: &str| {
        log.lines()
            .find(|l| l.starts_with("[hot-text] "))
            .unwrap_or_else(|| panic!("the link reports the default order:\n{log}"))
            .split(':')
            .next()
            .unwrap()
            .to_string()
    };
    assert!(script(&gnu_log).ends_with(".v1.ld"), "{gnu_log}");
    assert!(script(&lld_log).ends_with(".v1.lld.ld"), "{lld_log}");
    assert_eq!(run(&lld, None), run(&gnu, None));
    let symbols = symbol_addresses(&lld);
    let at = |name: &str| symbols.iter().find(|s| s.2 == name).map(|s| s.0);
    let (start, end) = (
        at("__perry_rt_hot_text_start").expect("hot run start"),
        at("__perry_rt_hot_text_end").expect("hot run end"),
    );
    let inside = symbols
        .iter()
        .filter(|s| matches!(s.1.as_str(), "t" | "T") && start <= s.0 && s.0 < end)
        .count();
    assert!(inside >= 200, "{inside} functions in the hot run");
}
