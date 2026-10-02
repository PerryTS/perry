//! `super.m` is a property lookup on the home object's CURRENT
//! `[[Prototype]]` (`C.prototype`'s in an instance method, `C`'s in a static
//! one) with `this` as the receiver, and `inst instanceof K` walks inst's live
//! chain for `K.prototype`. The program and node's output are the
//! `one_shape_class_super_relink` fixture.
//!
//! `super.m()` called the body the compiler resolved along the declared
//! `extends` chain, and its runtime fallback resolved through the declared
//! vtable, so a patched, deleted or accessor parent method never applied
//! (#11760). `typeof super.m` and `super.m` as a value were the declared
//! method's wrapper, `super.x` read the declared parent's prototype even
//! after a relink, and a static `super.s()` resolved an instance method of
//! the same name. `instanceof` walked declared class ids, and
//! `C.prototype.__proto__ = X` was compiled as a prototype-method install
//! named `__proto__` (#11765).

use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = include_str!("../../../tests/fixtures/one_shape_class_super_relink/main.ts");
const EXPECTED: &str =
    include_str!("../../../tests/fixtures/one_shape_class_super_relink/expected.txt");

const STATIC_SOURCE: &str =
    include_str!("../../../tests/fixtures/one_shape_class_super_relink/static_super.ts");
const STATIC_EXPECTED: &str =
    include_str!("../../../tests/fixtures/one_shape_class_super_relink/static_super.expected.txt");

/// Compiles `source` into `dir` and returns the executable.
fn compile(dir: &std::path::Path, source: &str) -> PathBuf {
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

#[test]
fn static_super_is_not_resolved_from_instance_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), STATIC_SOURCE);
    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "binary failed ({:?})\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    let wrong: Vec<String> = STATIC_EXPECTED
        .lines()
        .zip(stdout.lines())
        .filter(|(want, got)| want != got)
        .map(|(want, got)| format!("got `{got}`, node `{want}`"))
        .collect();
    assert!(
        wrong.is_empty() && STATIC_EXPECTED.lines().count() == stdout.lines().count(),
        "{wrong:#?}\n{stdout}"
    );
}

#[test]
fn super_and_instanceof_follow_the_live_prototype_chain() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = compile(dir.path(), SOURCE);
    // Runtime trip counts: the primed cases change the chain mid-loop, and a
    // fixed small loop would be unrolled. The output does not depend on n.
    for n in ["40", "41"] {
        let run = Command::new(&output)
            .arg(n)
            .current_dir(dir.path())
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_POISON_FROMSPACE", "1")
            .output()
            .expect("run compiled binary");
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(
            run.status.success(),
            "n={n}: binary failed ({:?})\nstderr:\n{}",
            run.status,
            String::from_utf8_lossy(&run.stderr)
        );
        let mut wrong: Vec<String> = EXPECTED
            .lines()
            .zip(stdout.lines())
            .filter(|(want, got)| want != got)
            .map(|(want, got)| format!("got `{got}`, node `{want}`"))
            .collect();
        if EXPECTED.lines().count() != stdout.lines().count() {
            wrong.push(format!(
                "{} lines, node {}",
                stdout.lines().count(),
                EXPECTED.lines().count()
            ));
        }
        assert!(wrong.is_empty(), "n={n}: {wrong:#?}\n{stdout}");
    }
}
