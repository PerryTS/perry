//! Object-literal accessors (`{ get k() {}, set k(v) {} }`) on the web target.

use super::run_web_source;

// A literal's getter and setter run with the object as `this`. Expected output is Node's.
#[test]
fn an_object_literal_accessor_is_called() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {
  base: 2,
  get double() { return this.base * 2; },
  set half(v: number) { this.base = v / 2; },
};
console.log(o.double);
o.half = 10;
console.log(o.base, o.double);
"#
        ),
        "4\n5 10"
    );
}

// A pair is one property at its first key's position, a later definition of the key
// replaces the earlier one, and a key is converted once. Expected output is Node's.
#[test]
fn an_object_literal_accessor_is_a_property_like_nodes() {
    assert_eq!(
        run_web_source(
            r#"
const sym = Symbol("s");
let n = 1;
const k = { toString: () => "ck" };
const o: any = {
  a: 1,
  get pair() { return n; }, set pair(v: number) { n = v; },
  get [sym]() { return "sym"; },
  get [k as any]() { return "computed"; },
  get 7() { return "num"; },
  get data() { return "lost"; }, data: 5,
  gone: 6, get gone() { return "won"; },
  z: 2,
};
o.pair = 4;
o.pair += 3;
console.log(o.pair, o[sym], o.ck, o[7], o.data, o.gone);
console.log(Object.keys(o).join(","), JSON.stringify(o));
const d: any = Object.getOwnPropertyDescriptor(o, "pair");
console.log(typeof d.get, typeof d.set, d.enumerable, d.configurable, "value" in d);
console.log(Object.getOwnPropertyDescriptor(o, "data")!.value, Object.getOwnPropertyDescriptor(o, "z")!.value);
const copy: any = Object.assign({}, o);
console.log(copy.pair, "get" in Object.getOwnPropertyDescriptor(copy, "pair")!);
"#
        ),
        "7 sym computed num 5 won\n7,a,pair,ck,data,gone,z {\"7\":\"num\",\"a\":1,\"pair\":7,\"ck\":\"computed\",\"data\":5,\"gone\":\"won\",\"z\":2}\nfunction function true true false\n5 2\n7 false"
    );
}

// The receiver is the object the property is read from, `super` starts at the literal's prototype,
// and a method's captured `this` is not the accessor's. Expected output is Node's.
#[test]
fn an_object_literal_accessor_binds_its_receiver() {
    assert_eq!(
        run_web_source(
            r#"
const proto: any = { who: "proto", get me() { return this.who; }, set me(v: string) { this.who = v; } };
const o: any = Object.create(proto);
o.who = "o";
console.log(o.me, proto.me);
o.me = "set";
console.log(o.who, proto.who, Object.keys(o).join(","));
const sup: any = { __proto__: proto, who: "sup", get me() { return "super " + super.me; } };
console.log(sup.me);
class C { v = 3; make() { const self = this; return { v: 1, get own() { return this.v; }, get outer() { return self.v; }, get arrow() { return (() => this.v)(); } }; } }
const made = new C().make();
console.log(made.own, made.outer, made.arrow);
"#
        ),
        "o proto\nset proto who\nsuper sup\n1 3 1"
    );
}

// An accessor's exception reaches the reader's `catch`, and a computed key's reaches the literal's. Expected output is Node's.
#[test]
fn an_object_literal_accessor_throws_to_its_caller() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = { get bad() { throw new RangeError("get"); }, set bad(v: any) { throw new RangeError("set " + v); }, get only() { return 1; } };
try { o.bad; console.log("no throw"); } catch (e: any) { console.log(e.name, e.message); }
try { o.bad = 2; console.log("no throw"); } catch (e: any) { console.log(e.name, e.message); }
try { const p: any = { get [(() => { throw new Error("key"); })()]() { return 1; } }; console.log(p); } catch (e: any) { console.log(e.message); }
"#
        ),
        "RangeError get\nRangeError set 2\nkey"
    );
}

// An async body builds and uses the same accessors. Expected output is Node's.
#[test]
fn an_object_literal_accessor_in_an_async_body() {
    assert_eq!(
        run_web_source(
            r#"
async function f(x: number) {
  let hits = 0;
  const o: any = { x, get twice() { hits++; return this.x * 2; }, set twice(v: number) { hits++; this.x = v / 2; } };
  const before = o.twice;
  await null;
  o.twice = 20;
  let msg = "none";
  const t: any = { get bad() { throw new RangeError("async"); } };
  try { t.bad; } catch (e: any) { msg = e.message; }
  return [before, o.x, o.twice, hits, msg, Object.keys(o)].join(",");
}
f(3).then((v) => console.log(v));
"#
        ),
        "6,10,20,3,async,x,twice"
    );
}

// Each evaluation of the literal has its own captures. Expected output is Node's.
#[test]
fn an_object_literal_accessor_keeps_its_captures() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) {
  let v = tag;
  return { get cur() { return v; }, set cur(next: string) { v = next; } };
}
const a = make("a"), b = make("b");
a.cur = "a2";
console.log(a.cur, b.cur);
const { cur } = a;
console.log(cur, "cur" in b, b["cur"]);
const fn: any = { get add() { return (p: number, q: number) => p + q; } };
console.log(fn.add(1, 2));
console.log(a);
"#
        ),
        "a2 b\na2 true b\n3\n{ cur: [Getter/Setter] }"
    );
}
