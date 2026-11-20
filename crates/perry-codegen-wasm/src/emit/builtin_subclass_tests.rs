//! A user class extending a built-in whose instances hold internal state, on the web
//! target.

use super::run_web_source;

// An instance of a class extending Map, Set, WeakMap, WeakSet, Array or Date is that
// built-in object, initialized by `super(...)` or the implicit constructor, so the
// built-in's methods and getters work on it and its initial entries pass through an
// overridden `add`. Expected output is Node's.
#[test]
fn a_subclass_of_a_stateful_builtin_is_that_builtin() {
    assert_eq!(
        run_web_source(
            r#"
class M extends Map<string, number> {}
class S extends Set<number> {}
class Doubled extends Set<number> { add(v: number) { return super.add(v * 2); } }
class Counted extends Map<string, number> { constructor() { super([["init", 1]]); this.set("x", 5); } extra() { return this.size * 10; } }
class Deeper extends M {}
class A extends Array<number> {}
class D extends Date {}
class W extends WeakMap<object, number> {}
class WS extends WeakSet<object> {}
const m = new M([["p", 1]]); m.set("a", 2);
const s = new S([1, 2]); s.add(2); s.add(3);
const c = new Counted();
const d = new Deeper([["q", 9]]);
const a = new A(); a.push(4); a.push(5);
const k = {}; const w = new W([[k, 7]]);
console.log(m.size + " " + m.get("a") + " " + m.has("p") + " " + Array.from(m.keys()).join(",") + " " + (m instanceof M) + " " + (m instanceof Map));
console.log(Array.from(new Doubled([1, 2])).join(","));
console.log(s.size + " " + s.has(3) + " " + c.size + " " + c.extra() + " " + c.get("init") + " " + d.size + " " + d.get("q") + " " + (d instanceof M));
console.log(a.length + " " + a.join("-") + " " + Array.isArray(a) + " " + new D(0).getUTCFullYear() + " " + w.get(k) + " " + w.has({}) + " " + new WS([k]).has(k));
"#
        ),
        "2 2 true p,a true true\n2,4\n3 true 2 20 1 1 9 true\n2 4-5 true 1970 7 false true"
    );
}
