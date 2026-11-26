//! `call`, `apply` and `bind` on a closure on the web target.

use super::{run_web_source, run_web_ui_source_then};

// A closure inherits from Function.prototype and has call / apply / bind: the receiver
// and arguments reach the closure, a bound function has Node's `bound <name>` and
// `length`, binds again, constructs with `new`, and serves as a callback, in a
// synchronous and an async body. Expected output is Node's.
#[test]
fn closures_call_apply_and_bind() {
    assert_eq!(
        run_web_source(
            r#"
const add = function (this: any, a: number, b: number): number { return (this ? this.k : 0) + a + b; };
function plain(a: number, b: number, c: number) { return a + b + c; }
const arrow = (x: number, y: number) => x * y;
const o = { k: 100 };
console.log(typeof add.bind, typeof add.call, typeof add.apply, add instanceof Function, Object.create(add) instanceof Function);
console.log(add.call(o, 1, 2), add.apply(o, [3, 4]), arrow.call(null, 3, 4), arrow.apply(null, [5, 6]), plain.apply(null, [1, 2, 3]));
const b = add.bind(o, 1);
console.log(typeof b, b(2), b.name, b.length, plain.bind(null, 1).name, plain.bind(null, 1)(2, 3), arrow.bind(null, 1, 2, 3).length);
const bb = b.bind(null, 5);
console.log(bb(), bb.name, bb.length);
const Pt = function (this: any, x: number, y: number) { this.x = x; this.y = y; };
const BP: any = Pt.bind(null, 7);
const p = new BP(8);
console.log(p.x, p.y);
console.log([1, 2, 3].map(plain.bind(null, 10, 20)).join(","), typeof String(add), (arrow + "!").endsWith("!"));
async function inAsync(): Promise<void> {
  const c = add.bind(o, 10);
  console.log("async", c(5), add.call(o, 1, 1), add.apply(o, [2, 2]), c.name, c.length);
}
inAsync();
"#
        ),
        "function function function true true\n103 107 12 30 6\nfunction 103 bound add 1 bound plain 6 0\n106 bound bound add 0\n7 8\n31,32,33 string true\nasync 115 102 104 bound add 1"
    );
}

// `apply` reads an array-like's `length` as ToLength does. Expected output is Node's.
#[test]
fn apply_reads_an_array_like_length_as_to_length() {
    assert_eq!(
        run_web_source(
            r#"
const f = function (...a: number[]) { return a.length; };
let e = "none";
try { f.apply(null, { length: 2 ** 32 } as any); } catch (x: any) { e = x.name; }
console.log(f.apply(null, { length: -1 } as any), f.apply(null, { length: NaN } as any), f.apply(null, { length: "2", 0: 1, 1: 2 } as any), f.apply(null, { length: 2.7 } as any), e);
"#
        ),
        "0 0 2 2 RangeError"
    );
}

// A bound function is a host callback: timers and promise reactions call it, bound once
// or rebound. Expected output is Node's.
#[test]
fn timers_and_promises_call_a_bound_function() {
    assert_eq!(
        run_web_source(
            r#"
const o = { k: 5 };
const g = function (this: any, a: number) { console.log("timeout", this.k, a); };
setTimeout(g.bind(o, 1), 0);
setTimeout(g.bind(o).bind(null, 2), 0);
const h = function (this: any) { console.log("interval", this.k); clearInterval(iv); };
const iv = setInterval(h.bind(o), 0);
const f = function (this: any, a: number, b: number) { return this.k * 100 + a * 10 + (b | 0); };
Promise.resolve(4).then(f.bind(o, 7)).then((v: any) => console.log("then", v));
Promise.resolve(6).then(f.bind(o, 7).bind(null, 3)).then((v: any) => console.log("then2", v));
"#
        ),
        "then 574\nthen2 573\ntimeout 5 1\ntimeout 5 2\ninterval 5"
    );
}

// An array method calls a rebound function as it calls a bound one. Expected output is
// Node's.
#[test]
fn array_methods_call_a_rebound_function() {
    assert_eq!(
        run_web_source(
            r#"
const o = { k: 5 };
const f = function (this: any, a: number, b: number) { return this.k * 100 + a * 10 + (b | 0); };
const twice = (x: number) => x * 2;
const b2 = f.bind(o, 7).bind(null, 3);
let seen = 0;
[1, 2].forEach(twice.bind(null).bind(null, 5) as any);
[4, 5].forEach(((x: number) => { seen += x; }).bind(null).bind(null));
console.log([1, 2].map(b2).join(","), [1, 2].map(twice.bind(null).bind(null)).join(","), [1, 2, 3].filter(((x: number) => x > 1).bind(null).bind(null)).length, [1, 2].some(((x: number) => x > 1).bind(null).bind(null)), seen);
"#
        ),
        "573,573 2,4 2 true 9"
    );
}

// `new` on a bound function makes an instance only when its target is a constructor: a
// bound arrow, async or generator function throws. Expected output is Node's.
#[test]
fn new_on_a_bound_non_constructor_throws() {
    assert_eq!(
        run_web_source(
            r#"
const tryNew = (C: any) => { try { return typeof new C(1); } catch (e: any) { return e.name; } };
const A = (x: number) => x;
const AF = async function () {};
const G = function* () {};
const F: any = function (this: any, x: number) { this.x = x; };
const BF: any = F.bind(null);
console.log(tryNew(A.bind(null)), tryNew(AF.bind(null)), tryNew(G.bind(null)), tryNew(BF), new BF(9).x);
"#
        ),
        "TypeError TypeError TypeError object 9"
    );
}

// A UI callback may be a bound function; it gets the event's plain value.
#[test]
fn ui_callbacks_call_a_bound_function() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Button, Toggle } from "perry/ui";
const o = { k: 5 };
const onTap = function (this: any) { console.log("tap", this.k); };
const onToggle = function (this: any, tag: string, on: boolean) { console.log("toggle", this.k, tag, on); };
Button("B", onTap.bind(o));
Toggle("T", onToggle.bind(o, "t").bind(null) as any);
"#,
            r#"
[...uiHandles.values()].find(e => e.tagName === "BUTTON").dispatch("click");
const t = [...uiHandles.values()].find(e => e._inp);
t._inp.checked = true;
t._inp.dispatch("change");
"#
        ),
        "tap 5\ntoggle 5 t true"
    );
}
