//! Each call creates its own `arguments` object holding that call's
//! argument values (ECMA-262 10.2.11 FunctionDeclarationInstantiation,
//! 10.4.4 CreateUnmappedArgumentsObject). A strict function that reads
//! `arguments` is summarized like any other: each run births its own
//! object, so `arguments[i]` of one call never reads another call's
//! argument, and its `this` and result stay the caller's own. (three.js
//! `Object3D.add` reads `arguments.length` / `arguments[i]`: one shared body
//! made every child's `parent` every receiver.) A sloppy function's
//! mapped object aliases its parameters and stays one shared body.

use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.cts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let cfg = FactsConfig::default();
    crate::compute(&[(path, &hir)], &cfg)
}

fn escape_of(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.cts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.cts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

/// The receiver set of the one `.key` read site of the program's own code.
fn recv_of(pf: &ProgramFacts, key: &str) -> String {
    let d = pf.dump_text();
    let pat = format!(" .{key} call=");
    let l = d
        .lines()
        .find(|l| l.starts_with("S ") && l.contains(&pat))
        .unwrap_or_else(|| panic!("no site .{key}\n{d}"));
    l.split(" recv={").nth(1).and_then(|r| r.split('}').next()).unwrap_or_default().to_string()
}

const HEAD: &str = "const unknown: any = new Function(process.argv[2])();
     class Node3 {
       kids: any = null;
       parent: any = null;
       add(o: any): any {
         if (arguments.length > 1) {
           for (let i = 0; i < arguments.length; i++) this.add(arguments[i]);
           return this;
         }
         this.kids.push(o);
         o.parent = this;
         return this;
       }
     }
     class P1 extends Node3 { p1 = 1; kids: any[] = []; }
     class P2 extends Node3 { p2 = 1; kids: any[] = []; }
     class X { xx = 1; }
     class Y { yy = 1; }
     const a = new P1();
     const b = new P2();
     const x = new X();
     const y = new Y();
";

// The receivers are observed at read sites, not by escaping one of them:
// an escaped receiver escapes its prototype chain, `Node3.prototype` (which
// holds `add`) included, and unknown code may then replace `add` for every
// receiver. Each class makes its own `kids` array (one array literal
// shared by both classes would hold both children).

/// `y.parent` holds only `b`, `x.parent` only `a`.
#[test]
fn each_child_parent_is_only_its_own_receiver() {
    let pf = whole(&format!("{HEAD}\n a.add(x); b.add(y); console.log(y.parent.p2, x.parent.p1);"));
    assert_eq!(recv_of(&pf, "p2"), "t.cts:P2", "{}", pf.dump_text());
    assert_eq!(recv_of(&pf, "p1"), "t.cts:P1", "{}", pf.dump_text());
}

/// `b.kids` holds only `y`.
#[test]
fn each_receiver_kids_hold_only_its_own_child() {
    let pf = whole(&format!("{HEAD}\n a.add(x); b.add(y); console.log(b.kids[0].yy);"));
    assert_eq!(recv_of(&pf, "yy"), "t.cts:Y", "{}", pf.dump_text());
}

/// The result of `b.add(y)` is only `b`.
#[test]
fn each_call_result_is_only_its_own_receiver() {
    let pf = whole(&format!("{HEAD}\n const r = a.add(x); const q = b.add(y); console.log(q.p2, r.p1);"));
    assert_eq!(recv_of(&pf, "p2"), "t.cts:P2", "{}", pf.dump_text());
    assert_eq!(recv_of(&pf, "p1"), "t.cts:P1", "{}", pf.dump_text());
}

/// Two children in one call: `arguments[i]` reads each argument of this
/// call (the loop's recursive `this.add`), and only those.
#[test]
fn every_argument_of_one_call_is_added() {
    let pf = whole(&format!("{HEAD}\n a.add(x, x); b.add(y); console.log(x.parent.p1, b.kids[0].yy);"));
    assert_eq!(recv_of(&pf, "p1"), "t.cts:P1", "{}", pf.dump_text());
    assert_eq!(recv_of(&pf, "yy"), "t.cts:Y", "{}", pf.dump_text());
}

const SUM: &str = "const unknown: any = new Function(process.argv[2])();
     class N1 { m1 = 1; }
     class N2 { m2 = 1; }
     class K1 { n = new N1(); }
     class K2 { n = new N2(); }
     class S {
       static sum(): any { let s: any = 0; for (const v of arguments) s = v.n; return s; }
     }
";

/// Iterating `arguments` yields only this call's arguments. Open: the
/// iterator tokens of a run are not born (their own nodes carry the
/// lookup-made `ThisTo`/`BuiltinCallee`/`Raise`), and with them born the
/// values `next` reads still reach every run's result.
#[test]
#[ignore = "open: for-of over a run's own array is not per run"]
fn iterated_arguments_are_only_the_calls_own() {
    let pf = whole(&format!("{SUM}\n const p = S.sum(new K1()); unknown[0] = S.sum(new K2()); console.log(p.m1);"));
    assert!(escape_of(&pf, "m2").is_some(), "{}", pf.dump_text());
    assert_eq!(escape_of(&pf, "m1"), None, "{}", pf.dump_text());
}

/// Control: an `arguments` object stored to a module variable outlives the
/// call; what it holds is what code reading the variable sees.
#[test]
fn stored_arguments_hold_every_stored_call() {
    let pf = whole(
        "const unknown: any = new Function(process.argv[2])();
         class A { a = 1; } class B { b = 1; }
         let keep: any = null;
         class Q { static f(o: any): any { keep = arguments; return 0; } }
         Q.f(new A()); Q.f(new B()); unknown[0] = keep;",
    );
    assert!(escape_of(&pf, "a").is_some(), "{}", pf.dump_text());
    assert!(escape_of(&pf, "b").is_some(), "{}", pf.dump_text());
}

/// Control: `arguments` handed to unknown code escapes its elements.
#[test]
fn arguments_passed_to_unknown_code_escape() {
    let pf = whole(
        "const unknown: any = new Function(process.argv[2])();
         class C { c = 1; } class D { d = 1; }
         class G { static g(o: any): any { unknown(arguments); return 0; } }
         G.g(new C()); console.log(new D().d);",
    );
    assert!(escape_of(&pf, "c").is_some(), "{}", pf.dump_text());
    assert_eq!(escape_of(&pf, "d"), None, "{}", pf.dump_text());
}

/// Control: a sloppy function's mapped `arguments[0]` is its parameter: the
/// write is read back through `p` (10.4.4.7 CreateMappedArgumentsObject).
#[test]
fn a_mapped_write_reaches_the_parameter() {
    let pf = whole(
        "const unknown: any = new Function(process.argv[2])();
         class A { a = 1; } class Z { z = 1; }
         function m(p: any) { arguments[0] = new Z(); return p; }
         unknown[0] = m(new A());",
    );
    assert!(escape_of(&pf, "z").is_some(), "{}", pf.dump_text());
}

/// Control: a function unknown code calls is walked with unknown
/// arguments, and every escaped function is active.
#[test]
fn an_escaped_arguments_reader_takes_unknown_arguments() {
    let pf = whole(
        "const unknown: any = new Function(process.argv[2])();
         class W { w = 1; } class V { v = 1; }
         class E { static e(): any { arguments[0].w = new W(); return 0; } }
         unknown.f = E.e; E.e(new V());",
    );
    assert!(escape_of(&pf, "w").is_some(), "{}", pf.dump_text());
    assert!(pf.dump_text().contains("# escaped-inactive=0\n"), "{}", pf.dump_text());
}
