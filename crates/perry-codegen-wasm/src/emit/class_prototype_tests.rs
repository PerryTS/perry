//! A declared class's prototype, shared by its instances and its class value, on the web
//! target.

use super::run_web_source;

// A declared class's instances inherit from its class value's `prototype`, which
// inherits from the parent class's and holds the class's own methods, so a method added
// to a parent's prototype later reaches every instance. Expected output is Node's.
#[test]
fn instances_and_class_prototypes_form_one_chain() {
    assert_eq!(
        run_web_source(
            r#"
class A { m() { return "a"; } }
class B extends A { n() { return "b"; } }
class C extends B {}
class E extends Error {}
const b: any = new B();
const c: any = new C();
const before = [b.__proto__ === B.prototype, b.__proto__.__proto__ === A.prototype, c.__proto__.__proto__.__proto__ === A.prototype, b.constructor === B, "m" in b, Object.keys(B.prototype).length, (B.prototype as any).hasOwnProperty === undefined];
(A.prototype as any).added = function () { return "added"; };
console.log(before.join(",") + " | " + b.added() + " " + c.added() + " " + b.m() + c.n() + " | " + (b instanceof A) + " " + (new E() instanceof Error));
"#
        ),
        "true,true,true,true,true,0,false | added added ab | true true"
    );
}

// The shared prototype has `constructor` before any read of the class, Symbol-keyed
// methods, and an instance computed method defined after a static one made the class
// value. Expected output is Node's.
#[test]
fn a_class_prototype_holds_constructor_and_every_computed_method() {
    assert_eq!(
        run_web_source(
            r#"
class P {}
const p: any = new P();
const pc = p.constructor;
class It { [Symbol.iterator]() { return [][Symbol.iterator](); } }
const s = "s", m = "m";
class K { static [s]() { return 1; } [m]() { return "m"; } }
console.log((pc === P) + " " + pc.name + " " + Object.keys(p).length + " | " + (Symbol.iterator in new It()) + " " + (m in new K()));
"#
        ),
        "true P 0 | true true"
    );
}
