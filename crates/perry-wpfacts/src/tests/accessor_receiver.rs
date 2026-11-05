//! A class accessor's body runs on the receiver of the read or store that
//! reached it. A summarized body reads and writes through its instance's
//! `this` term; the accessor group fed only the shared node, so the
//! instance ran with no receiver: a setter's `this.h = v` stored nowhere
//! and a getter's `this.x` read nothing (whole program and per unit).

use super::*;

fn kind<'a>(pf: &'a ProgramFacts, class: &str, key: &str) -> &'a crate::derive::KindFact {
    &field(tmpl(pf, class), key).kind
}

// Node: prints `7`.
#[test]
fn a_setter_stores_through_its_receiver() {
    let pf = facts(
        "class P { lm = 7; }
         class S { h: any = 0; set host(v: any) { this.h = v; } }
         const s = new S();
         s.host = new P();
         console.log(s.h.lm);",
    );
    let k = kind(&pf, "S", "h");
    assert!(
        k.objs.iter().any(|d| d.ends_with(":P")),
        "S.h {k:?}\n{}",
        pf.dump_text()
    );
}

// Node: prints `7`.
#[test]
fn a_getter_reads_through_its_receiver() {
    let pf = facts(
        "class P { lm = 7; }
         class G { x: any = 0; get v() { return this.x; } }
         class B { f: any = 0; }
         const g = new G();
         g.x = new P();
         const b = new B();
         b.f = g.v;
         console.log(b.f.lm);",
    );
    let k = kind(&pf, "B", "f");
    assert!(
        k.objs.iter().any(|d| d.ends_with(":P")),
        "B.f {k:?}\n{}",
        pf.dump_text()
    );
}
