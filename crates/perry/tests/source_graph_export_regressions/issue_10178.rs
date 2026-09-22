//! A CJS cycle must observe module.exports replacements before init finishes.

use super::{compile_and_run_output, write};

#[test]
fn esbuild_getters_are_callable_and_live_through_a_require_cycle() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "token.cjs",
        include_str!("../../../../test-files/cjs_esbuild_cycle/token.cjs"),
    );
    write(
        dir.path(),
        "consumer.cjs",
        include_str!("../../../../test-files/cjs_esbuild_cycle/consumer.cjs"),
    );
    write(
        dir.path(),
        "main.mjs",
        &include_str!("../../../../test-files/test_cjs_esbuild_cycle.ts")
            .replace("./cjs_esbuild_cycle/token.cjs", "./token.cjs"),
    );
    let run = compile_and_run_output(dir.path(), "main.mjs");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "function:function:true\n0\n42\n52\n2\n"
    );
    assert!(
        run.stderr.is_empty(),
        "cycle emitted a warning: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn class_static_getter_is_visible_in_a_comparator_first_cycle() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "comparator.cjs",
        "const ANY = Symbol('ANY');\n\
         class Comparator { static get ANY() { return ANY; } }\n\
         module.exports = Comparator;\n\
         const Range = require('./range.cjs');\n\
         Comparator.read = function () { return Range.read(); };\n",
    );
    write(
        dir.path(),
        "range.cjs",
        "class Range { static read() { return Comparator.ANY; } }\n\
         module.exports = Range;\n\
         const Comparator = require('./comparator.cjs');\n",
    );
    write(
        dir.path(),
        "main.mjs",
        "import Comparator from './comparator.cjs';\n\
         console.log(typeof Comparator.read());\n\
         console.log(Comparator.read() === Comparator.ANY);\n",
    );
    let run = compile_and_run_output(dir.path(), "main.mjs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "symbol\ntrue\n");
    assert!(
        run.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn missing_property_in_a_cycle_still_warns() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "a.cjs",
        "exports.before = true;\n\
         const b = require('./b.cjs');\n\
         exports.seen = b.seen;\n\
         exports.after = true;\n",
    );
    write(
        dir.path(),
        "b.cjs",
        "const a = require('./a.cjs');\nexports.seen = a.after;\n",
    );
    write(
        dir.path(),
        "main.mjs",
        "import a from './a.cjs';\nconsole.log(a.seen);\n",
    );
    let run = compile_and_run_output(dir.path(), "main.mjs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "undefined\n");
    assert!(String::from_utf8_lossy(&run.stderr).contains(
        "Accessing non-existent property 'after' of module exports inside circular dependency"
    ));
}

#[test]
fn deferred_property_read_in_a_cycle_does_not_warn() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "a.cjs",
        "const b = require('./b.cjs');\n\
         exports.after = 42;\n\
         exports.read = b.read;\n",
    );
    write(
        dir.path(),
        "b.cjs",
        "const a = require('./a.cjs');\n\
         exports.read = function () { return a.after; };\n",
    );
    write(
        dir.path(),
        "main.mjs",
        "import a from './a.cjs';\nconsole.log(a.read());\n",
    );
    let run = compile_and_run_output(dir.path(), "main.mjs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    assert!(
        run.stderr.is_empty(),
        "deferred property read emitted a warning: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}
