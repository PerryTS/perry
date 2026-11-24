//! Rest parameters on the web target.

use super::run_web_source;

// A rest parameter receives the arguments from its position on as one array: in a function
// declaration, an arrow, an `any`-typed arrow, an instance, a static and a computed method,
// a generator, an array, promise and timer callback and an async function, and from a call
// in an async body.
// Expected output is Node's.
#[test]
fn a_rest_parameter_receives_its_arguments_as_an_array() {
    assert_eq!(
        run_web_source(
            r#"
function count(...r: number[]) { return r.length; }
function first(a: number, ...r: number[]) { return a + ":" + r.join(","); }
function anyRest(...r: any[]) { return r.length + "|" + r.join(","); }
const arrow = (...r: number[]) => r.length;
const anyArrow: any = (a: number, ...r: number[]) => a + "/" + r.join(",");
class K { m(...r: number[]) { return r.length; } static s(a: number, ...r: number[]) { return a + r.length; } }
async function countAsync(...r: number[]) { return r.length; }
console.log([count(1, 2, 3), count(), first(1, 2, 3), first(1), anyRest("a", 2), arrow(1, 2, 3), arrow(), anyArrow(1, 2, 3), anyArrow(1), new K().m(1, 2, 3), K.s(10, 1, 1)].join(" "));
console.log([1, 2].map((...r: number[]) => r.length).join(","));
countAsync(1, 2, 3).then((v) => console.log("async " + v));
const key = "c" + "";
class C2 { [key](...r: number[]) { return r.length; } }
function* gen(...r: number[]) { yield r.length; }
let seen = 0;
[7].forEach((...r: any[]) => { seen = r.length; });
console.log([(new C2() as any).c(1, 2, 3), gen(1, 2, 3).next().value, seen].join(" "));
async function fromAsync(xs: number[]) { return count(1, 2, 3) + " " + first(1) + " " + count(...xs); }
fromAsync([4, 5]).then((v) => console.log("from async " + v));
Promise.resolve(7).then((...r: any[]) => console.log("then " + r.length));
setTimeout((...r: any[]) => console.log("timer " + r.length), 0);
"#
        ),
        "3 0 1:2,3 1: 2|a,2 3 0 1/2,3 1/ 3 12\n3,3\n3 3 3\nasync 3\nfrom async 3 1: 2\nthen 1\ntimer 0"
    );
}

// A constructor's rest parameter receives the arguments from its position on as one array:
// on `new`, through an inherited (synthesized) constructor, from an explicit `super(...)`,
// through a class value, for a class declared in a function and on `new` in an async body.
// Expected output is Node's.
#[test]
fn a_constructor_rest_parameter_receives_its_arguments_as_an_array() {
    assert_eq!(
        run_web_source(
            r#"
class P { n: number; a: number[]; constructor(first: number, ...rest: number[]) { this.n = rest.length; this.a = rest; } }
class OnlyRest { a: number[]; constructor(...rest: number[]) { this.a = rest; } }
class D extends P {}
class E extends P { constructor(x: number) { super(x, x + 1, x + 2); } }
const p = new P(1, 2, 3, 4);
console.log(p.n, p.a.join(","), new P(1).n, new OnlyRest().a.length, new OnlyRest(5, 6).a.join(","));
console.log(new D(1, 2, 3).a.join(","), new E(7).a.join(","));
const Q: any = P;
console.log(new Q(1, 2, 3, 4).a.join(","));
function local(v: number) { class L { r: number[]; constructor(...r: number[]) { this.r = r; } } return new L(v, v); }
console.log(local(9).r.join(","));
async function inAsync() { return new OnlyRest(1, 8, 9).a.join(",") + " " + new P(1, 8, 9).a.join(",") + " " + new OnlyRest().a.length + new P(1).n + new D(1).n; }
inAsync().then((v) => console.log("async " + v));
"#
        ),
        "3 2,3,4 0 0 5,6\n2,3 8,9\n2,3,4\n9,9\nasync 1,8,9 8,9 000"
    );
}

// `super(...args)` runs the parent's constructor with the spread arguments: a mixed list,
// into a parent rest parameter, before the derived class's field initializers, into a
// built-in Error or Map parent, and from the constructor synthesized for a derived class
// that captures a variable. Expected output is Node's.
#[test]
fn super_with_spread_arguments_runs_the_parent_constructor() {
    assert_eq!(
        run_web_source(
            r#"
class Base { a: number; b: string; constructor(a: number, b: string) { this.a = a; this.b = b; } }
class D1 extends Base { constructor(...args: [number, string]) { super(...args); } }
class D2 extends Base { constructor(n: number) { const rest: [string] = ["y"]; super(n, ...rest); } }
class R { first: number; more: number[]; constructor(first: number, ...more: number[]) { this.first = first; this.more = more; } }
class S extends R { constructor(...xs: number[]) { super(...xs); } }
class F extends Base { tag = this.a * 10; constructor(...xs: [number, string]) { super(...xs); } }
class E extends Error { constructor(...xs: [string]) { super(...xs); } }
class M extends Map<string, number> { constructor(...xs: any[]) { super(...xs); } }
function make(k: number) {
  class B { v: number; constructor(v: number) { this.v = v; } }
  class C extends B { get k() { return k; } }
  return new C(3);
}
const d1 = new D1(1, "x"), d2 = new D2(7), s = new S(1, 2, 3), f = new F(4, "f");
console.log(d1.a, d1.b, d2.a, d2.b, s.first, s.more.join(","), f.tag, f.b);
const e = new E("boom"), m = new M([["k", 1]]), c = make(9);
console.log(e.message, e instanceof Error, m.get("k"), m.size, c.v, c.k);
"#
        ),
        "1 x 7 y 1 2,3 40 f\nboom true 1 1 3 9"
    );
}
