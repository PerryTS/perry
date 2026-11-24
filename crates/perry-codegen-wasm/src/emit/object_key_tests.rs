//! An object used as a property key on the web target.

use super::run_web_source;

// ToPropertyKey runs a conversion method that is a Perry closure, with the key object as
// `this`. Expected output is Node's.
#[test]
fn an_object_key_is_converted_by_its_closures() {
    assert_eq!(
        run_web_source(
            r#"
const sym = Symbol("s");
const o: any = { kk: 1, 5: "five" };
o[sym] = "symv";
const viaThis: any = { n: "kk", toString() { return this.n; } };
const viaValueOf: any = { toString: () => ({}), valueOf: () => "kk" };
const viaToPrimitive: any = {};
viaToPrimitive[Symbol.toPrimitive] = (hint: string) => (hint === "string" ? "kk" : "no");
const viaSymbol: any = {};
viaSymbol[Symbol.toPrimitive] = () => sym;
const inherited: any = Object.create({ toString: () => "kk" });
class Field { toString = () => "kk"; }
class Method { toString() { return "kk"; } }
console.log([o[viaThis], o[viaValueOf], o[viaToPrimitive], o[viaSymbol], o[inherited], o[new Field() as any], o[new Method() as any], o[{ toString: () => 5 } as any], o[{ a: 1 } as any]].join(","));
"#
        ),
        "1,1,1,symv,1,1,1,five,"
    );
}

// Every operation that takes a property key converts it. Expected output is Node's.
#[test]
fn every_keyed_operation_converts_an_object_key() {
    assert_eq!(
        run_web_source(
            r#"
const k: any = { toString: () => "kk" };
const mk = (): any => ({ kk: 1, m(x: number) { return x + 1; } });
const m: any = { toString: () => "m" };
const set = mk(); set[k] = 9;
const del = mk(); delete del[k];
const inc = mk(); inc[k]++;
const def = mk(); Object.defineProperty(def, k, { value: 3 });
const lit: any = { ...{}, [k]: 4 };
const { [k]: destructured } = mk();
class A { m() { return 6; } static [k] = 7; }
class B extends A { [k]() { return 8; } q() { return (super[m as "m"] as any)(); } }
const parent: any = { kk: 10 };
const child: any = { __proto__: parent, q() { return super[k as "kk"]; } };
console.log([set.kk, del.kk, inc.kk, def.kk, lit.kk, destructured, k in mk(), mk()[m](2), mk()[m](...[3])].join(","));
console.log([(A as any)[k], (new B() as any).kk(), child.q(), Object.getOwnPropertyDescriptor(mk(), k)!.value, mk().hasOwnProperty(k), mk().propertyIsEnumerable(k)].join(","));
"#
        ),
        "9,,2,3,4,1,true,3,4\n7,8,10,1,true,true"
    );
}

// An async body converts the key as a sync one does. Expected output is Node's.
#[test]
fn an_async_body_converts_an_object_key() {
    assert_eq!(
        run_web_source(
            r#"
const k: any = { toString: () => "kk" };
const m: any = { toString: () => "m" };
const o: any = { kk: 1, m(x: number) { return x + 1; } };
async function f() {
  const out: any[] = [o[k], k in o, o[m](2), o[m](...[3]), o.hasOwnProperty(k)];
  out.push(Object.getOwnPropertyDescriptor(o, k)!.value);
  o[k] = 9;
  out.push(o.kk);
  delete o[k];
  out.push("kk" in o);
  return out.join(",");
}
f().then((v) => console.log(v));
"#
        ),
        "1,true,3,4,true,1,9,false"
    );
}

// A conversion method a class getter returns is read through the getter. Expected output
// is Node's.
#[test]
fn an_object_key_reads_its_conversion_method_through_a_class_getter() {
    assert_eq!(
        run_web_source(
            r#"
class Got { n = "kk"; get toString(): any { return () => this.n; } }
class Throws { get toString(): any { throw new RangeError("getter"); } }
const o: any = { kk: 1 };
let msg = "none";
try { o[new Throws() as any]; } catch (e: any) { msg = e.message; }
console.log([o[new Got() as any], msg].join(","));
"#
        ),
        "1,getter"
    );
}

// An own property shadows a class getter, and a deleted method is not found. Expected
// output is Node's.
#[test]
fn an_object_key_conversion_method_lookup_follows_the_current_object() {
    assert_eq!(
        run_web_source(
            r#"
class Got { get toString(): any { throw new RangeError("unexpected"); } }
const own: any = new Got();
Object.defineProperty(own, "toString", { value: () => "kk" });
class Key { toString() { return "kk"; } valueOf() { return "vv"; } }
const key: any = new Key();
const o: any = { kk: 1, vv: 2, "[object Object]": 3 };
const before = o[key];
delete (Key.prototype as any).toString;
console.log([o[own], before, o[key]].join(","));
"#
        ),
        "1,1,3"
    );
}

// A key that cannot be converted throws, after a nullish receiver does. Expected output
// is Node's.
#[test]
fn an_unconvertible_object_key_throws() {
    assert_eq!(
        run_web_source(
            r#"
const bad: any = { toString: () => { throw new RangeError("boom"); } };
const unconvertible: any = { toString: () => ({}), valueOf: () => ({}) };
const uncallable: any = {};
uncallable[Symbol.toPrimitive] = 1;
const out: string[] = [];
try { ({} as any)[bad]; } catch (e: any) { out.push(e.message); }
try { ({} as any)[unconvertible]; } catch (e: any) { out.push(e.message); }
try { ({} as any)[uncallable]; } catch (e: any) { out.push(e.name); }
try { (null as any)[bad]; } catch (e: any) { out.push(e.message); }
async function f() { return (null as any)[bad]; }
async function g() { return ({} as any)[bad]; }
console.log(out.join(";"));
f().catch((e) => console.log(e.name)).then(() => g()).catch((e) => console.log(e.message));
"#
        ),
        "boom;Cannot convert object to primitive value;TypeError;Cannot read properties of null\nTypeError\nboom"
    );
}
