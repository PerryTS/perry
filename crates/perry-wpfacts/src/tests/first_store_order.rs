//! LIFE-C3: a class's birth keys follow the constructor's first store of
//! each key, a store in a branch, loop or `try` at its place in the text,
//! the order the codegen layout (`own_keys`) and the class key index read
//! from the same HIR reservations. A key a run may skip is no definite key:
//! it is a reservation, its slot known and its presence not.

use super::*;

#[test]
fn birth_keys_follow_the_first_store_in_the_text() {
    let pf = facts(
        r#"
        class IfElse { constructor(i: number) { this.raw = 1; if (i % 2) { this.pre = [i]; } else { this.pre = []; } this.build = []; } }
        class Tried { constructor(f: boolean) { this.a = 1; try { if (f) throw 1; this.ok = 1; } catch (e) { this.err = 1; } this.z = 2; } }
        class OneArm { constructor(t: boolean) { this.p = 1; if (t) { this.opt = 1; } this.q = 2; } }
        class Base { constructor(c: boolean) { if (c) { this.bk = 1; } this.bt = 1; } }
        class Sub extends Base { constructor(c: boolean) { super(c); for (let i = 0; i < 2; i++) { this.sk = i; } this.st = 2; } }
        const a = new IfElse(1); const b = new Tried(false); const c = new OneArm(false); const d = new Sub(true);
        console.log(a.build, a.pre, b.z, c.q, c.opt, d.st, d.bk);
    "#,
    );
    let keys = |n: &str| tmpl(&pf, n).birth_keys.clone();
    assert_eq!(keys("IfElse"), ["raw", "pre", "build"], "{}", pf.dump_text());
    assert_eq!(keys("Tried"), ["a", "ok", "err", "z"], "{}", pf.dump_text());
    assert_eq!(keys("OneArm"), ["p", "opt", "q"], "{}", pf.dump_text());
    assert_eq!(keys("Sub"), ["bk", "bt", "sk", "st"], "{}", pf.dump_text());
    // `opt` may be absent while `q` is present: it is no definite key but a
    // reservation. Its slot is its own word in every object (the birth
    // allocates it), holding its value or the hole: every read pairs the
    // load with the hole compare and a hole is the generic operation, so the
    // slot holds whatever order the run adds it in (an add after `q` keeps
    // the hole and goes to the overflow, which orders enumeration). `q`'s
    // slot is fixed by the reserved one before it, and the template's layout
    // is not the complete birth layout.
    let one = tmpl(&pf, "OneArm");
    let opt = field(one, "opt");
    assert!(!opt.definite, "{}", pf.dump_text());
    assert!(opt.reserved, "{}", pf.dump_text());
    assert_eq!(opt.slot, Some(1), "{}", pf.dump_text());
    let q = field(one, "q");
    assert!(q.definite && q.slot == Some(2), "{}", pf.dump_text());
    assert!(!one.fixed_layout, "{}", pf.dump_text());
    // `pre`, stored on both arms, is definite.
    assert!(field(tmpl(&pf, "IfElse"), "pre").definite, "{}", pf.dump_text());
}

/// A reservation an early `return` skips is no definite key (the field-init
/// phase creates only definitions; a reservation is created by its store).
#[test]
fn a_reservation_an_early_return_skips_is_not_definite() {
    let pf = facts(
        r#"
        class Early { constructor(f: boolean) { this.a = 1; if (f) return; this.b = 2; } }
        const e = new Early(true);
        console.log(e.a, e.b);
    "#,
    );
    assert!(field(tmpl(&pf, "Early"), "a").definite, "{}", pf.dump_text());
    assert!(!field(tmpl(&pf, "Early"), "b").definite, "{}", pf.dump_text());
}
