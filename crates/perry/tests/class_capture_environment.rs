//! Class captures live with the class definition, not on instances.
//!
//! A class nested in a function captures the enclosing function's locals.
//! When the class definition is evaluated at most once (module top, an IIFE
//! body, a function declaration called exactly once — `lower::run_once`) its
//! members read and write those captures in the class ENVIRONMENT
//! (`Expr::ClassEnvGet`/`ClassEnvSet`), the way V8 keeps them in the closure
//! context. Every other capturing class keeps the per-instance `__perry_cap_*`
//! snapshot, because each of its evaluations has its own environment.
//!
//! Each test is differential: Node runs the same source, and both outputs must
//! equal the expected text. Each also asserts WHICH storage the compiler chose
//! (`PERRY_CLASS_CAPTURE_DIAG`), so a test meant for the environment path
//! cannot pass on the instance path or the reverse.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Compile `src` with perry, run it and Node on it; returns
/// (perry stdout, node stdout, the compiler's capture-storage diagnostics).
fn run_both(src: &str) -> (String, String, Vec<String>) {
    run_both_with(src, &[])
}

/// [`run_both`] with extra `(file name, contents)` sources beside `main.ts`.
fn run_both_with(src: &str, extra: &[(&str, &str)]) -> (String, String, Vec<String>) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in extra {
        std::fs::write(dir.path().join(name), contents).expect("write extra source");
    }
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write fixture");
    let bin = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .env("PERRY_CLASS_CAPTURE_DIAG", "1")
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&bin)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert_success("perry compile", &compile);
    let diag: Vec<String> = String::from_utf8_lossy(&compile.stderr)
        .lines()
        .filter(|l| l.starts_with("[class-capture]"))
        .map(str::to_string)
        .collect();
    let run = Command::new(&bin)
        .current_dir(dir.path())
        .output()
        .expect("run compiled fixture");
    assert_success("compiled fixture", &run);
    let node = Command::new("node")
        .current_dir(dir.path())
        .arg(&entry)
        .output()
        .expect("run Node semantic oracle");
    assert_success("Node", &node);
    (
        String::from_utf8(run.stdout).expect("utf-8"),
        String::from_utf8(node.stdout).expect("utf-8"),
        diag,
    )
}

/// The storage the compiler reported for the capturing class `class` (a
/// named class expression registers as `<name>__class_expr_<n>`).
fn storage_of<'a>(diag: &'a [String], class: &str) -> &'a str {
    let exact = format!(" class={class} ");
    let expr = format!(" class={class}__class_expr_");
    let line = diag
        .iter()
        .find(|l| l.contains(&exact) || l.contains(&expr))
        .unwrap_or_else(|| panic!("no capture diagnostic for class {class}: {diag:#?}"));
    line.rsplit("storage=").next().expect("storage field")
}

fn check(src: &str, expected: &str, storage: &[(&str, &str)]) {
    check_with(src, &[], expected, storage)
}

fn check_with(src: &str, extra: &[(&str, &str)], expected: &str, storage: &[(&str, &str)]) {
    let (perry, node, diag) = run_both_with(src, extra);
    assert_eq!(node, expected, "Node disagrees with the expected text");
    assert_eq!(perry, expected, "perry disagrees with Node");
    for (class, want) in storage {
        assert_eq!(storage_of(&diag, class), *want, "storage of {class}");
    }
}

#[test]
fn method_mutation_is_shared_by_instances_and_the_enclosing_scope() {
    check(
        "(function () {
           let count = 0;
           class Counter { bump() { count++; return count; } read() { return count; } }
           const a = new Counter(), b = new Counter();
           a.bump(); a.bump(); b.bump();
           console.log(a.read(), b.read(), count);
           count = 10;
           console.log(a.read(), b.read());
         })();",
        "3 3 3\n10 10\n",
        &[("Counter", "env")],
    );
}

#[test]
fn a_constructor_write_reaches_the_enclosing_scope() {
    check(
        "(function () {
           let made = 0;
           const unit = 'px';
           class Shape { size: string; constructor() { made++; this.size = this.describe(); } describe() { return 'shape'; } }
           class Box extends Shape { describe() { return 'box' + unit + made; } }
           const bx = new Box();
           new Shape();
           console.log(bx.size, made);
         })();",
        "boxpx1 2\n",
        &[("Shape", "env"), ("Box", "env")],
    );
}

#[test]
fn an_extracted_method_reads_its_class_environment_with_any_this() {
    check(
        "(function () {
           const tag = 'T';
           class Tagged { get() { return tag + ':' + typeof this; } }
           const g = Tagged.prototype.get;
           console.log(g.call({}), g.call(42), new Tagged().get());
         })();",
        "T:object T:number T:object\n",
        &[("Tagged", "env")],
    );
}

#[test]
fn statics_and_instances_share_one_environment() {
    check(
        "(function () {
           let seq = 100;
           const label = 'L';
           class S {
             static next() { return ++seq; }
             static peek() { return label + seq; }
             inst() { return label + seq; }
           }
           S.next(); S.next();
           console.log(S.peek(), new S().inst(), seq);
         })();",
        "L102 L102 102\n",
        &[("S", "env")],
    );
}

#[test]
fn subclasses_and_super_read_their_own_class_environment() {
    check(
        "(function () {
           const base = 'B';
           class Base { who() { return base; } greet() { return 'hi ' + this.who(); } }
           const derived = 'D';
           class Derived extends Base { who() { return derived + '<' + super.who(); } own() { return derived; } }
           class Plain extends Base {}
           const d = new Derived();
           console.log(d.greet(), d.own(), new Plain().greet(), d instanceof Base);
         })();",
        "hi D<B D hi B true\n",
        &[("Base", "env"), ("Derived", "env")],
    );
}

#[test]
fn accessors_write_a_captured_primitive_for_every_instance() {
    check(
        "(function () {
           let stored = 'x';
           class Acc { get v() { return stored; } set v(n) { stored = n; } }
           const p = new Acc(), q = new Acc();
           p.v = 'y';
           console.log(q.v, stored);
         })();",
        "y y\n",
        &[("Acc", "env")],
    );
}

#[test]
fn a_capture_initialized_after_the_class_is_seen_by_earlier_instances() {
    check(
        "(function () {
           class Late { get() { return late; } }
           const early = new Late();
           const late = 7;
           let mode = 'a';
           function setMode(m) { mode = m; }
           const Mode = class { get() { return mode; } };
           const mo = new Mode();
           setMode('b');
           console.log(early.get(), mo.get(), new Mode().get());
         })();",
        "7 b b\n",
        &[("Late", "env")],
    );
}

#[test]
fn the_cjs_factory_shape_is_evaluated_once() {
    // `function f(){…} return f();` inside an IIFE is how the CommonJS wrapper
    // runs a module body.
    check(
        "const out = (function () {
           function factory() {
             const k = 3;
             class E { f = k * 2; m() { return k + this.f; } }
             return new E().m();
           }
           return factory();
         })();
         console.log(out);",
        "9\n",
        &[("E", "env")],
    );
}

#[test]
fn a_class_declaration_captures_per_call_of_its_function() {
    check(
        "function make(v) {
           class C { get() { return v; } set(n) { v = n; } }
           return new C();
         }
         const m1 = make(1), m2 = make(2);
         m1.set(5);
         console.log(m1.get(), m2.get());",
        "5 2\n",
        &[("C", "instance")],
    );
}

#[test]
fn a_class_expression_captures_per_evaluation() {
    check(
        // Every instance is built before any method runs, so a shared
        // environment (the last evaluation's) would answer for all of them.
        "function factory(tag) { return class Base { t() { return tag; } }; }
         const A = factory('a'), B = factory('b');
         class SubA extends A { t() { return 'sub' + super.t(); } }
         const a = new A(), b = new B(), s = new SubA();
         const perCall = [];
         for (const v of [1, 2, 3]) perCall.push(new (factory('x' + v))());
         console.log(a.t(), b.t(), s.t(), perCall.map((o) => o.t()).join(','));",
        "a b suba x1,x2,x3\n",
        &[("Base", "env-guarded")],
    );
}

#[test]
fn a_class_in_a_loop_body_is_not_a_single_evaluation() {
    check(
        "(function () {
           const made = [];
           for (const v of ['p', 'q']) {
             class L { get() { return v; } }
             made.push(new L());
           }
           console.log(made.map((o) => o.get()).join(','));
         })();",
        "p,q\n",
        &[("L", "instance")],
    );
}

#[test]
fn reflection_sees_only_declared_fields() {
    check(
        "(function () {
           const k = 1, tag = 'n';
           class Node2 { pos = 0; end = 5; kind = k; name() { return tag + this.kind; } }
           const n = new Node2();
           const forIn = [];
           for (const key in n) forIn.push(key);
           console.log(Object.keys(n).join(','), JSON.stringify(n), forIn.join(','),
             Object.getOwnPropertyNames(n).join(','), JSON.stringify({ ...n }), n.name());
         })();",
        "pos,end,kind {\"pos\":0,\"end\":5,\"kind\":1} pos,end,kind pos,end,kind {\"pos\":0,\"end\":5,\"kind\":1} n1\n",
        &[("Node2", "env")],
    );
}

#[test]
fn a_statically_constructed_class_expression_keeps_its_evaluation() {
    // `new C()` through the binding is a static construct; once `mk` has run
    // twice the second instance must be recorded as the second evaluation's.
    check(
        "function mk(v) { const C = class { get() { return v; } }; return new C(); }
         const one = mk(1), two = mk(2), three = mk(3);
         console.log(one.get(), two.get(), three.get());",
        "1 2 3\n",
        &[("C", "env-guarded")],
    );
}

const REEVAL_MODULE: &str = "globalThis.__evals = (globalThis.__evals || 0) + 1;
const tag = 'e' + globalThis.__evals;
var Box = class {
  get() { return tag; }
  static tagOf() { return tag; }
};
module.exports = { make: () => new Box(), Box: Box };
";

const REEVAL_DRIVER: &str = "const first = require('./m.js');
const a = first.make();
const a2 = new first.Box();
const key = Object.keys(require.cache).find((k) => k.endsWith('/m.js'));
delete require.cache[key];
const second = require('module').createRequire(__filename)(key);
const b = second.make();
const b2 = new second.Box();
const c = first.make();
module.exports = [first.Box === second.Box, a.get(), a2.get(), b.get(), b2.get(), c.get(),
  first.Box.tagOf(), second.Box.tagOf(), first.Box.prototype.get.call(b)].join(' ');
";

#[test]
fn a_re_evaluated_module_body_keeps_old_instances_on_their_evaluation() {
    // Deleting the cache entry and requiring again re-runs the CommonJS module
    // body (perry-runtime's module_require re-require of a loaded module), so
    // `Box` has two evaluations: instances, statics and the first
    // evaluation's extracted method must each see their own `tag`.
    check_with(
        "console.log(require('./driver.js'));",
        &[("m.js", REEVAL_MODULE), ("driver.js", REEVAL_DRIVER)],
        "false e1 e1 e2 e2 e1 e1 e2 e1\n",
        &[("Box", "env-guarded")],
    );
}
