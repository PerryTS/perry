//! A class created per evaluation (a "fresh" class: a class expression in a
//! function, or a declaration whose evaluation has its own environment) is an
//! ordinary class object:
//!
//! * `class D extends L` extends the L of that evaluation, not the one every
//!   evaluation shares (`ext-*` lines);
//! * the class object owns `length`, `name`, `prototype` and its static methods
//!   as real own properties, so reflection lists them and `delete` removes them
//!   from that evaluation's class alone (`own-*` lines);
//! * `String(C)` and `C.toString()` are the class source (`str-*` lines);
//! * every evaluation after a template's first is built from the template's
//!   class-object and prototype shapes, and still owns its statics, its
//!   prototype and its methods: distinct per evaluation, at home in it, and a
//!   delete or redefinition on one leaves the others alone (`tpl-*` lines).
//!
//! The program and node's output are the `fresh_class_object_semantics`
//! fixture. The second run forces every minor collection to evacuate: the
//! class object is created, given its own properties and filled with statics
//! while the collector may move it.

use std::path::PathBuf;
use std::process::Command;

const SOURCE: &str = include_str!("../../../tests/fixtures/fresh_class_object_semantics/main.ts");
const EXPECTED: &str =
    include_str!("../../../tests/fixtures/fresh_class_object_semantics/expected.txt");

fn run(extra_env: &[(&str, &str)]) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
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
    let mut command = Command::new(&output);
    command.current_dir(dir.path());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let run = command.output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "binary failed ({:?})\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn check(stdout: &str, group: &str) {
    let wrong: Vec<String> = EXPECTED
        .lines()
        .zip(stdout.lines())
        .filter(|(want, got)| want.starts_with(group) && want != got)
        .map(|(want, got)| format!("got `{got}`, node `{want}`"))
        .collect();
    assert!(wrong.is_empty(), "{group}: {wrong:#?}\n{stdout}");
    assert_eq!(
        EXPECTED.lines().count(),
        stdout.lines().count(),
        "line count differs\n{stdout}"
    );
}

#[test]
fn a_static_extends_reaches_the_evaluated_class() {
    check(&run(&[]), "ext-");
}

#[test]
fn a_class_object_owns_its_length_name_prototype_and_statics() {
    check(&run(&[]), "own-");
}

#[test]
fn a_fresh_class_stringifies_to_its_source() {
    check(&run(&[]), "str-");
}

#[test]
fn evaluations_built_from_the_template_shapes_own_their_members() {
    check(&run(&[]), "tpl-");
}

#[test]
fn the_whole_program_matches_node_while_every_minor_evacuates() {
    let stdout = run(&[
        ("PERRY_GC_FORCE_EVACUATE", "1"),
        ("PERRY_GC_POISON_FROMSPACE", "1"),
    ]);
    assert_eq!(EXPECTED, stdout);
}
