//! A load of an accessor whose getter the runtime declares, on a builtin
//! object whose realm holder locks the member (`loads::bobj_declared_getter`):
//! the getter's declared result (a RegExp's `flags` and `source`: String;
//! its flag getters: Boolean, or `undefined` on `RegExp.prototype` itself),
//! no longer an unknown value. Each case has its negative twin: a value the
//! program stores as an own property of the object, a getter a program
//! subclass defines, an accessor redefined on the prototype. The arm's
//! switch (`PERRY_FACTS_DECLARED_GETTER_RESULTS=0`) is an environment
//! read, measured on whole programs, not here.

use super::*;
use crate::model::prim;

/// A class `C` whose fields keep what the loads under test read.
fn kinds(body: &str) -> ProgramFacts {
    facts(&format!(
        "class C {{ f: any; s: any; g: any; d: any; constructor() {{ this.f = 0; this.s = 0; this.g = 0; this.d = 0; }} }}
        const c = new C();
        {body}
        console.log(c.f, c.s, c.g, c.d);"
    ))
}

fn prims(pf: &ProgramFacts, k: &str) -> (bool, u16) {
    let f = field(tmpl(pf, "C"), k);
    (f.kind.top, f.kind.prims)
}

const NUM0: u16 = prim::NUM; // the constructor's `0`

#[test]
fn regexp_accessors_read_their_declared_results() {
    let pf = kinds(
        "const re: any = /a+/g;
        c.f = re.flags; c.s = re.source; c.g = re.global; c.d = re.dotAll;",
    );
    assert_eq!(
        prims(&pf, "f"),
        (false, NUM0 | prim::STR),
        "{}",
        pf.dump_text()
    );
    assert_eq!(
        prims(&pf, "s"),
        (false, NUM0 | prim::STR),
        "{}",
        pf.dump_text()
    );
    for k in ["g", "d"] {
        assert_eq!(
            prims(&pf, k),
            (false, NUM0 | prim::BOOL | prim::UNDEF),
            "{k}: {}",
            pf.dump_text()
        );
    }
}

/// `flags` reads the eight flag properties through [[Get]]
/// (`reads(field(this, ..))`): an own getter the program installs on the
/// RegExp runs, so its body is reached and its result kept apart.
#[test]
fn flags_runs_the_receivers_flag_getters() {
    let pf = facts(
        "class C { n: any; constructor() { this.n = 0; } }
        const c = new C();
        const re: any = /a/;
        Object.defineProperty(re, 'sticky', { get() { c.n = 'ran'; return 1; } });
        console.log(re.flags, c.n);",
    );
    let (top, p) = (
        field(tmpl(&pf, "C"), "n").kind.top,
        field(tmpl(&pf, "C"), "n").kind.prims,
    );
    assert!(
        !top && p & prim::STR != 0,
        "the getter ran: {}",
        pf.dump_text()
    );
}

/// Twin: an own property of the RegExp (a data property, an own getter)
/// is read beside the prototype's declared getter.
#[test]
fn an_own_property_of_the_regexp_is_read_too() {
    let pf = kinds(
        "const re: any = /a/g;
        Object.defineProperty(re, 'global', { value: 'own', configurable: true });
        Object.defineProperty(re, 'source', { get() { return 42; }, configurable: true });
        c.g = re.global; c.s = re.source;",
    );
    let (gt, g) = prims(&pf, "g");
    assert!(
        gt || g & prim::STR != 0,
        "own data value: {}",
        pf.dump_text()
    );
    let (st, s) = prims(&pf, "s");
    assert!(
        st || s & prim::NUM != 0,
        "own getter result: {}",
        pf.dump_text()
    );
}

/// Twin: a program subclass's getter is its own, not the realm's.
#[test]
fn a_subclass_getter_is_the_programs() {
    let pf = kinds(
        "class Own extends RegExp { get global(): any { return 'sub'; } }
        const re: any = new Own('a', 'g');
        c.g = re.global;",
    );
    let (gt, g) = prims(&pf, "g");
    assert!(gt || g & prim::STR != 0, "{}", pf.dump_text());
}

/// Twin: an accessor redefined on `RegExp.prototype` (the runtime's image
/// lock refuses it; the solve still keeps what such a write could make the
/// load read: the opaque member beside the declared result).
#[test]
fn a_redefinition_on_the_prototype_keeps_the_written_value() {
    let pf = kinds(
        "Object.defineProperty(RegExp.prototype, 'global', { get() { return 'patched'; }, configurable: true });
        const re: any = /a/g;
        c.g = re.global;",
    );
    let (gt, g) = prims(&pf, "g");
    assert!(
        gt || g & !(NUM0 | prim::BOOL | prim::UNDEF) != 0,
        "{}",
        pf.dump_text()
    );
}
