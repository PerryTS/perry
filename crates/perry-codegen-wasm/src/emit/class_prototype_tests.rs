//! A declared class's prototype, shared by its instances and its class value, and the
//! prototype a plain function parent lends it, on the web target.

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

// `class A extends F` with a plain function: A's prototype inherits the `prototype` F
// had when the class was defined, and a later `F.prototype = x` does not change it.
// Expected output is Node's.
#[test]
fn a_class_extending_a_function_inherits_its_prototype() {
    assert_eq!(
        run_web_source(
            r#"
function F() {}
const o = (F as any).prototype;
o.greet = function () { return "hi"; };
// @ts-ignore
class A extends F {}
// @ts-ignore
class B extends A {}
(F as any).prototype = { later: true };
const ap: any = A.prototype;
console.log([ap.__proto__ === o, "greet" in ap, ap.greet(), (B.prototype as any).__proto__ === ap, "later" in ap].join(","));
"#
        ),
        "true,true,hi,true,false"
    );
}

// `class A extends F` where `F.prototype` is null: A's prototype has no prototype.
// Expected output is Node's.
#[test]
fn a_class_extending_a_function_with_a_null_prototype_has_a_null_base() {
    assert_eq!(
        run_web_source(
            r#"
function F() {}
(F as any).prototype = null;
// @ts-ignore
class A extends F {}
console.log(Object.getPrototypeOf(A.prototype) === null);
"#
        ),
        "true"
    );
}

// `super.m()` in a class extending a plain function reads the `prototype` captured when
// the class was defined, also from a subclass. Expected output is Node's.
#[test]
fn super_in_a_class_extending_a_function_reads_its_prototype() {
    assert_eq!(
        run_web_source(
            r#"
function F() {}
(F as any).prototype.greet = function () { return "hi"; };
// @ts-ignore
class A extends F { greet() { return super.greet() + "!"; } }
// @ts-ignore
class B extends A { greet() { return super.greet() + "?"; } }
const b: any = new B();
console.log(b.greet());
"#
        ),
        "hi!?"
    );
}

// `super.m()` from a class one or more levels below a function parent reads that
// function's captured `prototype` with the instance as `this`; a null one ends the
// lookup below the class that extends it. Expected output is Node's.
#[test]
fn super_below_a_function_parent_keeps_the_receiver() {
    assert_eq!(
        run_web_source(
            r#"
function F() {}
(F as any).prototype.hello = function (this: any) { return "hello " + this.name; };
// @ts-ignore
class A extends F { name = "a"; direct() { return super.hello(); } }
// @ts-ignore
class B extends A { name = "b"; hi() { return super.hello(); } }
function N() {}
(N as any).prototype = null;
// @ts-ignore
class M extends N {}
// @ts-ignore
class K extends M { probe() { return super.x; } }
const a: any = new A(), b: any = new B(), k: any = new K();
console.log([a.direct(), b.hi(), k.probe()].join(","));
"#
        ),
        "hello a,hello b,"
    );
}
