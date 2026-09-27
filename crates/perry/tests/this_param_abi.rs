//! This-as-a-parameter, stage 1: every JS body — compiled closure, value
//! wrapper, native builtin — is `double body(i64 callee, i64 this, double
//! a0, ...)` (`perry_abi::JS_BODY_*`), and every caller hands the body, as its
//! `this` parameter, exactly the receiver the implicit-`this` cell holds for
//! the call. Bodies still read the cell, so stage 1 changes no behavior; what
//! it must not do is let the parameter and the cell disagree.
//!
//! A `PERRY_THIS_WITNESS=1` build makes the prologue of every compiled body
//! that reads the cell compare the two (`js_this_param_witness`), and reports
//! `PERRY_THIS_WITNESS checks=N mismatches=M` at exit.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const ROUTES: &str = include_str!("fixtures/this_param_routes.ts");
/// node v26.5.1's output for `fixtures/this_param_routes.ts`.
const ROUTES_NODE: &str =
    "346350 -933,-25431,81808,320762,1233\nfunction,function,function,function";

struct Run {
    stdout: String,
    stderr: String,
    ll_dir: tempfile::TempDir,
    _dir: tempfile::TempDir,
}

/// Compile `source` with the witness on and the constructed IR saved, run it.
fn compile_and_run_witnessed(source: &str) -> Run {
    let dir = tempfile::tempdir().expect("tempdir");
    let ll_dir = tempfile::tempdir().expect("ll tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_THIS_WITNESS", "1")
        .env("PERRY_SAVE_LL", ll_dir.path())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "binary failed ({:?})\nstderr:\n{stderr}",
        run.status
    );
    Run {
        stdout: String::from_utf8_lossy(&run.stdout).trim().to_owned(),
        stderr,
        ll_dir,
        _dir: dir,
    }
}

/// `(checks, mismatches)` from the witness's exit line.
fn witness_counts(stderr: &str) -> (u64, u64) {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("PERRY_THIS_WITNESS checks="))
        .unwrap_or_else(|| panic!("no witness report on stderr:\n{stderr}"));
    let field = |name: &str| -> u64 {
        line.split_whitespace()
            .find_map(|w| w.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
            .unwrap_or_else(|| panic!("no {name} in {line:?}"))
    };
    (field("checks"), field("mismatches"))
}

/// Sabotage: the method-site hit (or any caller) passes `undefined` — or the
/// runtime funnel passes anything but the cell's receiver — as `this` -> the
/// witness names the body and `mismatches` is non-zero. Sabotage: the witness
/// build emits no check -> `checks` is 0 and the lower bound fails.
#[test]
fn every_call_route_passes_the_cell_receiver_as_the_this_parameter() {
    let run = compile_and_run_witnessed(ROUTES);
    assert_eq!(run.stdout, ROUTES_NODE, "stderr:\n{}", run.stderr);
    let (checks, mismatches) = witness_counts(&run.stderr);
    // 300 iterations of ~13 routes into cell-reading bodies: a witness that
    // did not run cannot pass for one that found nothing.
    assert!(
        checks >= 3000,
        "the witness checked only {checks} body entries\nstderr:\n{}",
        run.stderr
    );
    assert_eq!(
        mismatches, 0,
        "a caller passed a receiver other than the cell's\nstderr:\n{}",
        run.stderr
    );
}

/// Every symbol the module hands to a closure allocator or registers as a
/// closure body is DEFINED with the JS body ABI. Sabotage: a wrapper family
/// (e.g. `__perry_wrap_<fn>`) defined without `%js_this` -> named here.
#[test]
fn every_installed_body_is_defined_with_the_js_body_abi() {
    let run = compile_and_run_witnessed(ROUTES);
    assert_eq!(run.stdout, ROUTES_NODE, "stderr:\n{}", run.stderr);
    let ir = read_ir(run.ll_dir.path());
    let installed = installed_bodies(&ir);
    assert!(
        installed.len() >= 10,
        "found only {} installed bodies in the IR: {installed:?}",
        installed.len()
    );
    let mut checked = 0;
    let mut wrong = Vec::new();
    for name in &installed {
        let Some(params) = defined_params(&ir, name) else {
            continue; // defined in another module or the runtime
        };
        checked += 1;
        if !params.starts_with("i64 %this_closure, i64 %js_this") {
            wrong.push(format!("@{name}({params})"));
        }
    }
    assert!(
        checked >= 10,
        "only {checked} installed bodies are defined here"
    );
    assert!(
        wrong.is_empty(),
        "bodies installed as function objects without the JS body ABI:\n{}",
        wrong.join("\n")
    );
    // Every wrapper family this fixture exercises is among them.
    for family in ["@perry_closure_", "@__perry_wrap_"] {
        assert!(
            installed
                .iter()
                .any(|n| format!("@{n}").starts_with(family)),
            "no {family}* body installed: {installed:?}"
        );
    }
}

fn read_ir(dir: &Path) -> String {
    let mut ir = String::new();
    for entry in std::fs::read_dir(dir).expect("ll dir") {
        let path = entry.expect("ll entry").path();
        if path.extension().is_some_and(|e| e == "ll") {
            ir.push_str(&std::fs::read_to_string(&path).expect("read ll"));
            ir.push('\n');
        }
    }
    assert!(!ir.is_empty(), "PERRY_SAVE_LL wrote no IR");
    ir
}

/// Symbols passed as `ptr @X` to `js_closure_alloc*` / `js_register_closure_*`.
fn installed_bodies(ir: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for call in ["@js_closure_alloc", "@js_register_closure_"] {
        for (at, _) in ir.match_indices(call) {
            let rest = &ir[at..];
            let Some(open) = rest.find('(') else { continue };
            let Some(close) = rest[open..].find(')') else {
                continue;
            };
            for arg in rest[open + 1..open + close].split(',') {
                let Some(sym) = arg.trim().strip_prefix("ptr @") else {
                    continue;
                };
                let sym = sym.trim_matches('"').to_string();
                if !out.contains(&sym) {
                    out.push(sym);
                }
            }
        }
    }
    out
}

/// The parameter list of `define ... @name(...)`, if defined in `ir`.
fn defined_params(ir: &str, name: &str) -> Option<String> {
    let quoted = format!("@\"{name}\"(");
    let plain = format!("@{name}(");
    ir.lines()
        .filter(|l| l.starts_with("define "))
        .find_map(|l| {
            let at = l
                .find(&plain)
                .map(|i| i + plain.len())
                .or_else(|| l.find(&quoted).map(|i| i + quoted.len()))?;
            let rest = &l[at..];
            Some(rest[..rest.find(')')?].to_string())
        })
}
