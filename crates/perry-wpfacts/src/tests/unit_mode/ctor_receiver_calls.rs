//! DECISIONS 231(b)(c): a published class's receivers (the receiver token,
//! the receiver markers of its constructor and methods, and the argument
//! markers that stand only for them) read a key's own field as what this
//! unit's code stores under it into the class's instances and receivers,
//! with other units' store effects and, once an instance escaped, the
//! writes through ⊤; another unit's value (the keyed reach) only where the
//! key may hold one no such store makes. Node checks: qb9
//! /root/RCV/r7s/crc.node.mjs.
use super::*;

/// Per unit proves nothing the whole program does not, and every field of
/// `class` holds at least the kinds the whole program finds.
fn never_stronger(mods: &[(&str, &str)], class: &str) -> (ProgramFacts, ProgramFacts) {
    let (u, w) = (units_of(mods), whole_of(mods));
    let more: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        more.is_empty(),
        "per unit proves more: {more:?}\n{}",
        u.dump_text()
    );
    let (tu, tw) = (tmpl(&u, class), tmpl(&w, class));
    for fw in &tw.fields {
        if let Some(fu) = tu.fields.iter().find(|f| f.key == fw.key) {
            assert!(
                kinds_cover(fu, fw),
                "field {} {:?} vs whole {:?}\n{}",
                fw.key,
                fu.kind,
                fw.kind,
                u.dump_text()
            );
        }
    }
    (u, w)
}

const SVC: &str = r#"export class Svc { size = 3; seen = 0; onCreate(p: P) { this.seen = p.n; } }
export class P {
    svc: Svc;
    n = 0;
    constructor(s: Svc) { BODY }
    init() { this.n = this.svc.size; }
    total() { return this.svc.size + this.n; }
}"#;
const SUB: &str = r#"import { P, Svc } from "./p";
export class Q extends P { constructor(s: Svc) { super(s); } }"#;
const MAIN: &str = r#"import { Q } from "./b";
import { P, Svc } from "./p";
console.log(new Q(new Svc()).total(), new P(new Svc()).total());"#;

fn svc(body: &str, on_create: &str) -> String {
    SVC.replace("BODY", body)
        .replace("this.seen = p.n;", on_create)
}

/// The constructor's `this.init()` and `s.onCreate(this)` with `s` another
/// unit's value: the instance escapes, yet only a key some unit writes
/// through ⊤ is opened on it, so the class's own calls stay its methods.
#[test]
fn an_escaped_receiver_opens_only_the_keys_written_through_top() {
    let p = svc(
        "this.svc = s; this.init(); s.onCreate(this);",
        "this.seen = p.n;",
    );
    let mods = [("p", p.as_str()), ("b", SUB), ("m", MAIN)];
    let (u, _) = never_stronger(&mods, "P");
    assert!(
        proven_set(&u).iter().any(|s| s.ends_with(".init")),
        "{}",
        u.dump_text()
    );
}

/// Soundness: the unit that receives the instance writes its keys (`n`,
/// `init`, `svc`) through a parameter its unknown caller fills.
#[test]
fn writes_into_an_escaped_receiver_reach_the_class() {
    for on_create in [
        "p.n = \"x\" as any;",
        "(p as any).init = () => { p.n = \"y\" as any; };",
        "p.svc = { size: \"z\" } as any;",
    ] {
        let p = svc("this.svc = s; s.onCreate(this); this.init();", on_create);
        let mods = [("p", p.as_str()), ("b", SUB), ("m", MAIN)];
        never_stronger(&mods, "P");
    }
}

/// The constructor stores its parameter and calls through the stored
/// field (tsc's Project: `this.projectService.onProjectCreation(this)`):
/// the units that construct an instance run the call on what they passed
/// (DECISIONS 236), so the receiver does not escape and the class's reads
/// stay proven. The callee itself stays the callers' value here: the
/// receiver marker keeps its keyed reach (DECISIONS 179).
#[test]
fn a_constructor_calls_through_the_field_it_stored() {
    let p = svc("this.svc = s; this.svc.onCreate(this);", "this.seen = p.n;");
    let mods = [("p", p.as_str()), ("b", SUB), ("m", MAIN)];
    let (u, _) = never_stronger(&mods, "P");
    assert!(!u.broken.iter().any(|b| b == "p.ts:P"), "{}", u.dump_text());
    assert!(
        proven_set(&u).iter().any(|s| s.ends_with(".svc")),
        "{}",
        u.dump_text()
    );
}

const K: &str = r#"function plain(this: any) { this.n = 1; }
export class P {
    n = 0;
    k: any = plain;
    act() { this.n = 2; }
    go() { this.k(); this.act(); return this.n + 1; }
}"#;

/// Soundness: another unit stores a function under `k` of an instance (a
/// store effect): the class's `this.k()` runs it.
#[test]
fn another_units_store_into_an_instance_is_the_receivers_field() {
    let m = r#"import { P } from "./p";
const p = new P();
p.k = function (this: any) { this.n = "s"; };
console.log(p.go());"#;
    never_stronger(&[("p", K), ("m", m)], "P");
}

/// Soundness: a subclass constructor of another unit reassigns `k`.
#[test]
fn a_subclass_constructor_reassigning_a_key_is_seen() {
    let b = r#"import { P } from "./p";
export class Q extends P { constructor() { super(); this.k = function (this: any) { this.n = "q"; }; } }"#;
    let m = r#"import { Q } from "./b";
console.log(new Q().go());"#;
    never_stronger(&[("p", K), ("b", b), ("m", m)], "P");
}

/// Soundness (DECISIONS 231(d)): a subclass of another unit overrides
/// `act`: the class's `this.act()` may run it.
#[test]
fn a_subclass_override_of_a_method_is_seen() {
    let b = r#"import { P } from "./p";
export class Q extends P { act() { this.n = "o" as any; } }"#;
    let m = r#"import { Q } from "./b";
import { P } from "./p";
console.log(new Q().go(), new P().go());"#;
    never_stronger(&[("p", K), ("b", b), ("m", m)], "P");
}

/// Soundness (DECISIONS 231(b)): a helper's parameter first holds only the
/// receiver, then another unit's object joins it: the helper's `x.k()` and
/// `x.n` take that object's values.
#[test]
fn a_late_non_receiver_join_opens_the_argument() {
    let p = r#"function help(x: any) { x.k(); return x.n + 1; }
function plain(this: any) { this.n = 1; }
export class P {
    n = 0;
    k: any = plain;
    go() { return help(this); }
}
export function use(o: any) { return help(o); }"#;
    let m = r#"import { P, use } from "./p";
console.log(new P().go(), use({ n: 0, k() { this.n = "j"; } }));"#;
    let (u, w) = (
        units_of(&[("p", p), ("m", m)]),
        whole_of(&[("p", p), ("m", m)]),
    );
    let more: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        more.is_empty(),
        "per unit proves more: {more:?}\n{}",
        u.dump_text()
    );
}

/// DECISIONS 236: per unit in a thread with a deadline (an oscillating
/// census never ends), with the round count, never stronger than whole for
/// every class in `classes`, each class's fields covering the whole
/// program's kinds.
fn bounded(mods: &[(&'static str, &'static str)], classes: &[&str]) -> ProgramFacts {
    let owned: Vec<(&'static str, &'static str)> = mods.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(units_of(&owned));
    });
    let u = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("the per-unit rounds converge");
    let w = whole_of(mods);
    // The receivers and arguments the units publish only grow (fixture
    // (a): 9 rounds); an oscillation never ends (RCV7: 1,250+).
    assert!(u.rounds <= 12, "rounds {}\n{}", u.rounds, u.dump_text());
    let more: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        more.is_empty(),
        "per unit proves more: {more:?}\n{}",
        u.dump_text()
    );
    for class in classes {
        let (tu, tw) = (tmpl(&u, class), tmpl(&w, class));
        for fw in &tw.fields {
            if let Some(fu) = tu.fields.iter().find(|f| f.key == fw.key) {
                assert!(
                    kinds_cover(fu, fw),
                    "{class}.{} {:?} vs whole {:?}\n{}",
                    fw.key,
                    fu.kind,
                    fw.kind,
                    u.dump_text()
                );
            }
        }
    }
    u
}

const RA: &str = r#"export class P {
    svc: any;
    n: any = 0;
    constructor(s: any) { this.svc = s; this.svc.onCreate(this); }
    total() { return this.n + 1; }
}"#;

/// DECISIONS 236 (a): the constructor's `this.svc.onCreate(this)` runs in
/// the unit that holds the instance (b's `super(r)`), on what it stored
/// under `svc` (b's `Reg`), with `this` as b's instance: Reg's store into
/// its argument (`p.n = "r"`) is seen, and the receiver does not escape.
#[test]
fn a_call_through_a_receiver_field_runs_where_the_receiver_is_held() {
    let b = r#"import { P } from "./p";
export class Reg { last: any = null; onCreate(p: any) { this.last = p; p.n = "r"; } }
export class Q extends P { constructor(r: Reg) { super(r); } }
export function lastN(r: Reg) { return r.last.n; }"#;
    let m = r#"import { Q, Reg, lastN } from "./b";
const r = new Reg();
const q = new Q(r);
console.log(q.total(), lastN(r));"#;
    let u = bounded(&[("p", RA), ("b", b), ("m", m)], &["P", "Q", "Reg"]);
    assert!(!u.broken.iter().any(|b| b == "p.ts:P"), "{}", u.dump_text());
}

/// DECISIONS 236 (b): the receiver escaped before the call (unknown code
/// may have replaced `svc`): the call is unknown code's, its arguments
/// escape as before.
#[test]
fn a_call_through_an_escaped_receivers_field_escapes_its_arguments() {
    let p = r#"export class Box { v: any = 1; }
export const box = new Box();
export class P {
    svc: any;
    constructor(s: any) { this.svc = s; (globalThis as any).hook(this); this.svc.onCreate(this, box); }
}
export function rd() { return box.v + 1; }"#;
    let m = r#"import { P, rd } from "./p";
(globalThis as any).hook = (p: any) => { p.svc = { onCreate(_: any, o: any) { o.v = "x"; } }; };
class S { onCreate(_: any, _o: any) {} }
new P(new S());
console.log(rd());"#;
    bounded(&[("p", p), ("m", m)], &["Box"]);
}

/// DECISIONS 236 (c): a subclass constructor of another unit reassigns
/// `svc`: the class's calls through `this.svc` run both callees (Reg1's
/// numeric store and Reg2's string store into the receiver).
#[test]
fn a_subclass_reassigning_the_receiver_field_runs_both_callees() {
    let p = r#"export class Reg1 { onCreate(p: any) { p.n = 1; } }
export class P {
    svc: any;
    n: any = 0;
    constructor(s: any) { this.svc = s; this.svc.onCreate(this); }
    ping() { this.svc.onCreate(this); return this.n; }
}"#;
    let b = r#"import { P, Reg1 } from "./p";
class Reg2 { onCreate(p: any) { p.n = "two"; } }
export class Q extends P { constructor() { super(new Reg1()); this.svc = new Reg2(); } }"#;
    let m = r#"import { Q } from "./b";
const q = new Q();
console.log(q.n, q.ping(), q.n);"#;
    let u = bounded(&[("p", p), ("b", b), ("m", m)], &["P", "Q"]);
    assert!(!u.broken.iter().any(|b| b == "p.ts:P"), "{}", u.dump_text());
}
