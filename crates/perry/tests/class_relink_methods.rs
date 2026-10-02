//! `Object.setPrototypeOf(C.prototype, X)` on a declared class: the methods,
//! getters and setters declared in C's old parent classes are off the
//! instance chain. Reads, `in`, `Reflect.has`, calls (any-typed, class-typed,
//! `this.m()`) and `super.m()` inside C's methods must all answer from the new
//! chain, at sites that ran before the relink and at fresh ones; C's own
//! members and X's (including another class's declared methods) stay visible;
//! static members are unaffected. The program and node's output are the
//! `one_shape_class_relink_methods` fixture.
//!
//! Every walk over declared class members (the vtable lookups behind reads,
//! `in` and call dispatch) followed the parent class id registered at
//! declaration, and the compiler's dispatch tower and `super.m()` call the
//! ancestor's body it resolved along the declared `extends` chain.

use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = include_str!("../../../tests/fixtures/one_shape_class_relink_methods/main.ts");
const EXPECTED: &str =
    include_str!("../../../tests/fixtures/one_shape_class_relink_methods/expected.txt");

fn stat(stderr: &str, name: &str) -> u64 {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("[method-site]"))
        .unwrap_or_else(|| panic!("no [method-site] line in:\n{stderr}"));
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no {name} in {line}"))
}

fn compile(dir: &Path, source: &str) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn mismatches(expected: &str, stdout: &str) -> Vec<String> {
    let mut wrong: Vec<String> = expected
        .lines()
        .zip(stdout.lines())
        .filter(|(want, got)| want != got)
        .map(|(want, got)| format!("got `{got}`, node `{want}`"))
        .collect();
    if expected.lines().count() != stdout.lines().count() {
        wrong.push(format!(
            "{} lines, node {}",
            stdout.lines().count(),
            expected.lines().count()
        ));
    }
    wrong
}

#[test]
fn old_parent_members_leave_a_relinked_class_chain() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), SOURCE);
    // Runtime trip counts: a fixed small loop is unrolled and its site never
    // reached. The output does not depend on the count.
    for n in ["40", "41"] {
        let run = Command::new(&output)
            .arg(n)
            .current_dir(dir.path())
            .env("PERRY_METHOD_SITE_STATS", "1")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_POISON_FROMSPACE", "1")
            .output()
            .expect("run compiled binary");
        let stdout = String::from_utf8_lossy(&run.stdout);
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(
            run.status.success(),
            "n={n}: binary failed ({:?})\nstderr:\n{stderr}",
            run.status
        );
        let wrong = mismatches(EXPECTED, &stdout);
        assert!(wrong.is_empty(), "n={n}: stale members {wrong:?}\n{stdout}");
        // The class read sites must have primed, and hit, before the relinks,
        // or nothing here tested what they validate.
        assert!(
            stat(&stderr, "class_read_primes") > 0 && stat(&stderr, "class_read_hits") > 0,
            "n={n}: no class read entry primed and hit\n{stderr}"
        );
    }
}

/// The dispatch tower a call on an untyped receiver compiles to hard-codes, per
/// class id, the body that class inherits. Past eight implementors it has no
/// shape probe, and it ignored prototype surgery entirely: a redefined parent
/// method kept running the old body. Its arms now consult the same per-name
/// invalidation bytes as the narrow tower.
#[test]
fn wide_dispatch_tower_sees_a_redefined_parent_method() {
    let mut source = String::from("function callM(o: any): any { return o.m(); }\n");
    for i in 1..=10 {
        source.push_str(&format!(
            "class B{i} {{ m() {{ return {i}; }} }}\nclass C{i} extends B{i} {{}}\n"
        ));
    }
    source.push_str(
        "const i: any = new C1();\n\
         console.log(\"a\", callM(i));\n\
         Object.defineProperty(B1.prototype, \"m\", { value: function () { return 99; }, writable: true, configurable: true });\n\
         console.log(\"b\", callM(i), i.m());\n\
         const j: any = new C2();\n\
         console.log(\"c\", callM(j));\n\
         Object.setPrototypeOf(C2.prototype, { k: 7 });\n\
         try { console.log(\"d\", callM(j)); } catch (e) { console.log(\"d\", e instanceof TypeError); }\n",
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), &source);
    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "binary failed ({:?})\n{stdout}",
        run.status
    );
    // node 26.5.1
    let wrong = mismatches("a 1\nb 99 99\nc 2\nd true\n", &stdout);
    assert!(wrong.is_empty(), "{wrong:?}\n{stdout}");
}

/// A getter on the replacement super chain can return a Proxy with [[Call]].
/// Object-target proxies stay non-callable even if they carry an apply trap.
#[test]
fn relinked_super_calls_callable_proxies() {
    let source =
        include_str!("../../../tests/fixtures/one_shape_class_relink_methods/proxy_super.ts");
    let expected = include_str!(
        "../../../tests/fixtures/one_shape_class_relink_methods/proxy_super.expected.txt"
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), source);
    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled proxy super fixture");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "proxy super fixture failed ({:?})\n{stdout}\n{stderr}",
        run.status
    );
    let wrong = mismatches(expected, &stdout);
    assert!(wrong.is_empty(), "{wrong:?}\n{stdout}\n{stderr}");
}
