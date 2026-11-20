//! A class used as a value on the web target (`__classRef` in `wasm_runtime.js`).

use super::{run_web_source, run_web_source_then};

// `new` on a class value hands its arguments to the compiled constructor as Perry
// values: a closure argument stays a closure the array bridges can call.
#[test]
fn a_class_value_constructor_receives_a_closure_argument() {
    assert_eq!(
        run_web_source(
            r#"
class M { out: number[]; constructor(cb: (n: number) => number) { this.out = [1, 2].map(cb); } }
const MM: any = M;
console.log(new MM((n: number) => n * 10).out.join(","));
"#
        ),
        "10,20"
    );
}

// Host `instanceof` on a class value walks the class table, as compiled `instanceof` does.
#[test]
fn host_instanceof_on_a_class_value_walks_the_class_chain() {
    assert_eq!(
        run_web_source_then(
            r#"
class P { }
class Q extends P { }
const refs: any[] = [P, Q];
console.log(refs.length);
"#,
            r#"
const P = __classRef("P", -1), Q = __classRef("Q", -1);
console.log([__classInstance("Q") instanceof P, __classInstance("Q") instanceof Q,
  __classInstance("P") instanceof Q, ({}) instanceof P, 1 instanceof P].join(" "));
"#
        ),
        "2\ntrue true false false false"
    );
}

// `new` on a class value yields what [[Construct]] yields: the instance, unless the
// constructor returns an object. A bare `return;` keeps the initialized instance.
#[test]
fn a_class_value_constructor_returning_no_object_yields_the_instance() {
    assert_eq!(
        run_web_source(
            r#"
class E { v = 1; constructor(x: number) { if (x > 0) return; this.v = 2; } get() { return this.v; } }
class O { v = 1; constructor() { return { v: 5 } as any; } }
const EE: any = E, OO: any = O;
const e = new EE(1);
console.log(e.v + " " + e.get() + " " + (e instanceof E) + " " + new EE(0).v + " " + new OO().v + " " + (new OO() instanceof O));
"#
        ),
        "1 1 true 2 5 false"
    );
}

#[test]
fn a_class_value_holds_static_side_properties_and_a_prototype() {
    assert_eq!(
        run_web_source(
            r#"
class T {}
(T as any).key = "class";
console.log((T as any).key);
const k = "dyn";
(T as any)[k] = 2;
console.log((T as any)[k]);
const base = { tag: "base" };
class U {}
Object.setPrototypeOf(U, base);
console.log((U as any).tag);
"#
        ),
        "class\n2\nbase"
    );
}

// A subclass's class value has its parent's class value as [[Prototype]], so it
// inherits the parent's static side; a built-in parent leaves it a plain function.
// Expected output is Node's.
#[test]
fn a_subclass_value_inherits_from_its_parent_class_value() {
    assert_eq!(
        run_web_source(
            r#"
const k = "made";
class A { static [k]() { return "A"; } }
class B extends A {}
class C extends B {}
class Plain {}
class E extends Error {}
const a: any = A, b: any = B, c: any = C, p: any = Plain, e: any = E;
console.log((b.__proto__ === A) + " " + (c.__proto__ === B) + " " + (c.__proto__.__proto__ === A) + " " + (p.__proto__ === a.__proto__) + " " + (typeof e.__proto__));
console.log((k in b) + " " + (k in c) + " " + c[k]() + " " + Object.prototype.hasOwnProperty.call(c, k) + " " + (new C() instanceof A));
"#
        ),
        "true true true true function\ntrue true A false true"
    );
}

// A class expression evaluated in a function body links its class value to its
// user parent's, as a declared subclass does. Expected output is Node's.
#[test]
fn an_evaluated_subclass_value_inherits_from_its_parent_class_value() {
    assert_eq!(
        run_web_source(
            r#"
class A { static made() { return "A"; } }
function mk(): any { return class extends A {}; }
function pair(): any { class P { static made() { return "P"; } } return class extends P {}; }
const b: any = mk(), q: any = pair();
console.log((b.__proto__ === A) + " " + b.made() + " " + q.made() + " " + (new b() instanceof A));
"#
        ),
        "true A P true"
    );
}

// An evaluated subclass is not its parent: its own static method wins, and a
// static it sets stays its own. Expected output is Node's.
#[test]
fn an_evaluated_subclass_value_keeps_its_own_static_side() {
    assert_eq!(
        run_web_source(
            r#"
class A { static who() { return "A"; } static n = 1; }
function mk(): any { return class extends A { static who() { return "B"; } }; }
const b: any = mk();
b.n = 2;
console.log(b.who() + " " + A.n + " " + b.n + " " + b.__proto__.who());
"#
        ),
        "B 1 2 A"
    );
}

// An evaluated subclass reads its parent's static fields, which live in the
// parent's static table rather than on its class value. Expected output is Node's.
#[test]
fn an_evaluated_subclass_value_reads_its_parents_static_fields() {
    assert_eq!(
        run_web_source(
            r#"
class A { static x = 1; static m() { return "m"; } }
function mk(): any { return class extends A {}; }
const K: any = mk(), k = "x";
console.log(K.x + " " + K[k] + " " + K.m() + " " + (A as any)[k]);
K.x = 5;
console.log(K.x + " " + A.x);
"#
        ),
        "1 1 m 1\n5 1"
    );
}
