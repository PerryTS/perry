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

// A built-in subclass's own methods win over the built-in's, and `super(...)` builds
// the built-in's state as its constructor does: the adder is read once, a Map entry is
// any object with keys 0 and 1, Array elements are own data properties past an
// inherited setter, and a Date's time value is set without its `setTime` override.
// Expected output is Node's.
#[test]
fn a_builtin_subclass_keeps_its_overrides_and_the_builtins_construction() {
    assert_eq!(
        run_web_source(
            r#"
class A extends Array<number> { at(i: number) { return "mine" + i; } findLast() { return "fl"; } }
const a: any = new A(); a.push(1);
console.log(a.at(0) + " " + a.findLast() + " " + a.includes(1));
class M extends Map<string, number> {}
console.log(new M([{0: "key", 1: 7}] as any).get("key"));
try { new M([1] as any); } catch (e) { console.log((e as Error).constructor.name); }
class Once extends Map<string, number> { set(k: string, v: number) { (this as any).set = null; return super.set(k, v); } }
console.log(new Once([["p", 1], ["q", 2]]).size);
class D extends Date { setTime(v: number) { return 0; } }
console.log(new D(0).getTime());
class B extends Array<string> {}
Object.defineProperty(B.prototype, "0", { set(v) { console.log("setter"); }, configurable: true });
const b = new B("x", "y");
console.log(b.length + " " + Object.prototype.hasOwnProperty.call(b, "0"));
"#
        ),
        "mine0 fl true\n7\nTypeError\n2\n0\n2 true"
    );
}

// A class with field initializers but no declared constructor, or one below it, still
// forwards `new`'s arguments to the built-in's constructor before its fields run.
// Expected output is Node's.
#[test]
fn a_field_only_builtin_subclass_forwards_its_arguments() {
    assert_eq!(
        run_web_source(
            r#"
class F extends Map<string, number> { label = "m"; }
class G extends F { extra = 2; }
const f = new F([["k", 1]]);
const g = new G([["a", 1], ["b", 2]]);
console.log(f.size + " " + f.get("k") + " " + f.label + " " + g.size + " " + g.label + g.extra);
"#
        ),
        "1 1 m 2 m2"
    );
}

// The collection constructor reads its adder through a compiled getter too: a getter
// that throws stops construction, and one that returns a non-callable is a TypeError.
// Expected output is Node's.
#[test]
fn a_builtin_subclass_reads_its_adder_through_a_getter() {
    assert_eq!(
        run_web_source(
            r#"
class Blocked extends Map<string, number> { get set(): any { throw new Error("blocked"); } }
class NotFn extends Set<number> { get add(): any { return 5; } }
try { new Blocked([]); console.log("built"); } catch (e) { console.log((e as Error).message); }
try { new NotFn([1]); console.log("built"); } catch (e) { console.log((e as Error).constructor.name); }
"#
        ),
        "blocked\nTypeError"
    );
}
