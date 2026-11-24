//! Own-property reflection on the web target. Expected output is Node's.

use super::run_web_source;

#[test]
fn reflect_get_own_property_descriptor_and_descriptors() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = { x: 1 };
const g = () => 7;
Object.defineProperty(o, "g", { get: g });
const ds: any = Object.getOwnPropertyDescriptors(o);
let prim = "no", nul = "no";
try { Reflect.getOwnPropertyDescriptor(5 as any, "x"); } catch (e: any) { prim = e.name; }
try { Object.getOwnPropertyDescriptors(null as any); } catch (e: any) { nul = e.name; }
console.log(JSON.stringify(Reflect.getOwnPropertyDescriptor(o, "x")) + JSON.stringify(ds.x));
console.log([ds.g.get === g, Object.keys(ds).join("+"), Reflect.getOwnPropertyDescriptor(o, "y") === undefined, prim, nul].join(","));
async function f() {
  const d: any = Reflect.getOwnPropertyDescriptor(o, "g");
  return [d.get === g, Object.getOwnPropertyDescriptors(o).x.value, Reflect.has(o, "x"), Object.hasOwn(o, "z"), Reflect.ownKeys(o).join("+"), Object.getOwnPropertyNames(o).length].join(",");
}
f().then((v) => console.log("async " + v));
"#
        ),
        "{\"value\":1,\"writable\":true,\"enumerable\":true,\"configurable\":true}{\"value\":1,\"writable\":true,\"enumerable\":true,\"configurable\":true}\ntrue,x+g,true,TypeError,TypeError\nasync true,1,true,false,x+g,2"
    );
}

#[test]
fn reflect_define_property_reports_success() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {};
const ok = Reflect.defineProperty(o, "x", { get: undefined });
const d: any = Object.getOwnPropertyDescriptor(o, "x");
Object.defineProperty(o, "fixed", { value: 1 });
const g: any = {};
Reflect.defineProperty(g, "v", { get: () => 7, enumerable: true });
let prim = "no";
try { Reflect.defineProperty(1 as any, "x", { value: 1 }); } catch (e: any) { prim = e.name; }
console.log([ok, typeof d, "get" in d, d.get === undefined, Reflect.defineProperty(o, "fixed", { value: 2 }), g.v, prim].join(","));
async function f() { const a: any = {}; return [Reflect.defineProperty(a, "x", { value: 3 }), a.x].join(","); }
f().then((v) => console.log("async " + v));
"#
        ),
        "true,object,true,true,false,7,TypeError\nasync true,3"
    );
}

#[test]
fn own_keys_of_a_descriptor() {
    assert_eq!(
        run_web_source(
            r#"
const acc: any = {};
Object.defineProperty(acc, "x", { set: (v: any) => {} });
const d: any = Object.getOwnPropertyDescriptor(acc, "x");
const names = Object.getOwnPropertyNames(d);
const keys = Reflect.ownKeys(d);
let prim = "no";
try { Reflect.ownKeys(1 as any); } catch (e: any) { prim = e.name; }
console.log([Object.hasOwn(d, "get"), Reflect.has(d, "get"), Object.hasOwn(d, "value"), Reflect.has(d, "toString"), Object.hasOwn(d, "toString")].join(","));
console.log([names.length, keys.length, keys.join("+"), prim].join(","));
"#
        ),
        "true,true,false,true,false\n4,4,get+set+enumerable+configurable,TypeError"
    );
}

// The runtime's own bookkeeping keys are not the program's properties.
#[test]
fn own_keys_hide_runtime_internals() {
    assert_eq!(
        run_web_source(
            r#"
function q() {}
(q as any).mine = 1;
const c = () => 1;
class A { x = 1; m() {} static s = 2; }
const K: any = class { y = 1; };
const kk = Reflect.ownKeys(new K());
console.log([Reflect.ownKeys(q).includes("mine"), Reflect.ownKeys(q).includes("funcIdx"), Reflect.ownKeys(c).includes("captures"), Object.hasOwn(q, "captures"), Object.getOwnPropertyDescriptor(q, "funcIdx") === undefined].join(","));
console.log([Reflect.ownKeys(new A()).join("+"), Reflect.ownKeys(A.prototype).length, kk.join("+"), Reflect.ownKeys(A).some((k) => typeof k === "symbol"), Reflect.ownKeys(K).some((k) => typeof k === "symbol"), Reflect.ownKeys(A).includes("s")].join(","));
"#
        ),
        "true,false,false,false,true\nx,2,y,false,false,true"
    );
}

// A wrapper-object key names the same property as its primitive, and a program's own key
// that only looks internal stays visible.
#[test]
fn wrapper_and_lookalike_keys() {
    assert_eq!(
        run_web_source(
            r#"
function q() {}
const look: any = { __perry_computed_field_key_user: 1 };
const o: any = { x: 1 };
const K: any = Object("x");
const d: any = Object.getOwnPropertyDescriptor(o, K);
console.log([Object.getOwnPropertyDescriptor(q, Object("captures") as any) === undefined, Object.hasOwn(q, Object("captures") as any), d.value, Object.hasOwn(o, K), Reflect.ownKeys(look).length, Object.hasOwn(look, "__perry_computed_field_key_user")].join(","));
"#
        ),
        "true,false,1,true,1,true"
    );
}

#[test]
fn a_function_declaration_has_a_name() {
    assert_eq!(
        run_web_source(
            r#"
function q() {}
async function aq() {}
const d: any = Object.getOwnPropertyDescriptor(q, "name");
console.log([d.value, d.writable, d.enumerable, d.configurable, q.name, (aq as any).name, Object.keys(q).length].join(","));
"#
        ),
        "q,false,false,true,q,aq,0"
    );
}

#[test]
fn json_stringify_omits_closures() {
    assert_eq!(
        run_web_source(
            r#"
const f = () => 1;
class R { static r(k: string, v: any): any { return k === "n" ? f : v; } }
const acc: any = {};
Object.defineProperty(acc, "x", { set: (v: any) => {}, configurable: true });
console.log(JSON.stringify({ n: 1, m: 2 }, R.r) + JSON.stringify(Object.getOwnPropertyDescriptor(acc, "x")));
console.log(JSON.stringify({ f, a: [f], n: 1 }) + JSON.stringify(f) + JSON.stringify({ f, n: 1 }, ["f", "n"]) + JSON.stringify(Object.create(f)));
console.log(JSON.stringify({ f, n: 1 }, (k, v) => (typeof v === "function" ? "fn" : v)) + JSON.stringify({ n: 1 }, (k, v) => (k === "n" ? f : v)));
console.log(JSON.stringify({ f }, (k, v) => (JSON.stringify({ f }) === "{}" && typeof v === "function" ? "fn" : v)) + ("toJSON" in f));
"#
        ),
        "{\"m\":2}{\"enumerable\":false,\"configurable\":true}\n{\"a\":[null],\"n\":1}undefined{\"n\":1}{}\n{\"f\":\"fn\",\"n\":1}{}\n{\"f\":\"fn\"}false"
    );
}

// `{ [k]: v }` defines a symbol or string key, in declaration order.
#[test]
fn a_computed_literal_key_is_an_own_property() {
    assert_eq!(
        run_web_source(
            r#"
const s = Symbol("k");
const k = "a";
const o: any = { z: 0, [s]: 1, [k]: 2, [k + "b"]: 3, [1 + 1]: 4 };
const d: any = Object.getOwnPropertyDescriptor(o, s);
const all: any = Object.getOwnPropertyDescriptors(o);
console.log([o[s], d.value, d.writable, d.enumerable, d.configurable, JSON.stringify(o), all[s].value].join(","));
console.log([Object.getOwnPropertySymbols(o).length, Object.getOwnPropertySymbols(o)[0] === s, Reflect.ownKeys(o).map((x) => String(x)).join("+"), Object.hasOwn(o, s)].join(","));
const p: any = { ["__proto__"]: 5, [k]: 1 };
console.log(Object.keys(p).join("+") + (Object.getPrototypeOf(p) === Object.prototype));
async function f() { const i: any = { [s]: 7, [k]: 8 }; return [i[s], i.a, typeof Object.getOwnPropertyDescriptor(i, s)].join(","); }
f().then((v) => console.log("async " + v));
"#
        ),
        "1,1,true,true,true,{\"2\":4,\"z\":0,\"a\":2,\"ab\":3},1\n1,true,2+z+a+ab+Symbol(k),true\n__proto__+atrue\nasync 7,8,object"
    );
}
