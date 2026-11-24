//! `Object.create` on the web target.

use super::run_web_source;

// `Object.create` builds an object on the given prototype, with or without property
// descriptors, in a synchronous and in an async body. Expected output is Node's.
#[test]
fn object_create_builds_an_object_on_the_prototype() {
    assert_eq!(
        run_web_source(
            r#"
const base: any = { k: 1 };
const o: any = Object.create(base);
o.own = 2;
const bare: any = Object.create(null);
bare.x = 1;
const withProps: any = Object.create(base, { p: { value: 5, enumerable: true } });
let threw = "no";
try { Object.create(5 as any); } catch (e: any) { threw = e.name; }
async function inAsync() { const a: any = Object.create(base); return a.k; }
console.log([typeof o, o.k, Object.keys(o).join("+"), typeof bare, bare.x, "toString" in bare, withProps.p, withProps.k, Object.keys(withProps).join("+"), threw].join(","));
inAsync().then((v) => console.log("async " + v));
"#
        ),
        "object,1,own,object,1,false,5,1,p,TypeError\nasync 1"
    );
}

// An accessor descriptor's closure runs with the receiver as `this`, and a getter
// that throws throws to the reader. Expected output is Node's.
#[test]
fn object_create_accessor_descriptors_run_closures() {
    assert_eq!(
        run_web_source(
            r#"
const arrow: any = Object.create(null, { x: { get: () => 7 } });
const o: any = Object.create({}, {
  g: { get: function (this: any) { return this.q; }, enumerable: true },
  s: { set: function (this: any, v: number) { this.t = v * 2; } },
  bad: { get: () => { throw new RangeError("boom"); } },
});
o.q = 9;
o.s = 3;
let threw = "no";
try { o.bad; } catch (e: any) { threw = e.message; }
class Props { static f = { get: () => 8 }; }
const fromClass: any = Object.create(null, Props as any);
const later: any = { y: { get: () => 4 } };
async function inAsync() { const a: any = Object.create(null, later); return a.y; }
console.log([arrow.x, o.g, o.t, threw, Object.keys(o).join("+"), fromClass.f].join(","));
inAsync().then((v) => console.log("async " + v));
"#
        ),
        "7,9,6,boom,g+q+t,8\nasync 4"
    );
}

// `Object.defineProperty` and `Object.defineProperties` accessors that are Perry
// closures run with the receiver as `this`. Expected output is Node's.
#[test]
fn define_property_accessor_descriptors_run_closures() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {};
Object.defineProperty(o, "x", { get: () => 7 });
Object.defineProperty(o, "s", { set: function (this: any, v: number) { this.t = v * 2; } });
Object.defineProperties(o, { g: { get: function (this: any) { return this.t + 1; } } });
o.s = 3;
const later: any = { get: () => 4 };
async function inAsync() { const a: any = {}; Object.defineProperty(a, "y", later); Object.defineProperties(a, { z: later }); return a.y + a.z; }
console.log([o.x, o.t, o.g].join(","));
inAsync().then((v) => console.log("async " + v));
"#
        ),
        "7,6,7\nasync 8"
    );
}

// The same closure redefines a non-configurable accessor, as the same host function
// does. Expected output is Node's.
#[test]
fn define_property_same_closure_redefines_an_accessor() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {};
const d: any = { get: () => 7 };
Object.defineProperty(o, "x", d);
Object.defineProperty(o, "x", d);
let threw = "no";
try { Object.defineProperty(o, "x", { get: () => 8 }); } catch (e: any) { threw = e.name; }
console.log([o.x, threw].join(","));
"#
        ),
        "7,TypeError"
    );
}

// A class instance is a descriptor whose fields its accessors give; a setter alone
// still makes its field present. Expected output is Node's.
#[test]
fn define_property_reads_a_class_instance_descriptor_through_its_getters() {
    assert_eq!(
        run_web_source(
            r#"
class Desc { get value() { return 1; } get enumerable() { return true; } }
const o: any = {};
Object.defineProperty(o, "x", new Desc() as any);
class Mixed { get value() { return 2; } set get(f: any) {} }
let threw = "no";
try { Object.defineProperty(o, "y", new Mixed() as any); } catch (e: any) { threw = e.name; }
console.log([o.x, Object.keys(o).join(""), "y" in o, threw].join(","));
"#
        ),
        "1,x,false,TypeError"
    );
}

// Each present field of a class-instance descriptor is read once. Expected output is Node's.
#[test]
fn define_property_reads_each_class_instance_descriptor_field_once() {
    assert_eq!(
        run_web_source(
            r#"
let reads = 0;
class D { get enumerable() { return true; } }
const d: any = new D();
Object.defineProperty(d, "value", { get() { return ++reads; }, enumerable: true });
const o: any = {};
Object.defineProperty(o, "x", d);
console.log([o.x, reads].join(","));
"#
        ),
        "1,1"
    );
}

// A descriptor's `get` is checked before its `set` is read. Expected output is Node's.
#[test]
fn define_property_rejects_a_class_instance_getter_before_reading_set() {
    assert_eq!(
        run_web_source(
            r#"
const seen: string[] = [];
class G { get get(): any { seen.push("get"); return 1; } get set(): any { seen.push("set"); return undefined; } }
let msg = "none";
try { Object.defineProperty({}, "x", new G() as any); } catch (e: any) { msg = e.name + ": " + e.message; }
console.log(msg + "|" + seen.join(","));
"#
        ),
        "TypeError: Getter must be a function: 1|get"
    );
}

// A mixed data and accessor descriptor throws before the next descriptor is read.
// Expected output is Node's.
#[test]
fn define_properties_rejects_a_mixed_descriptor_before_reading_the_next() {
    assert_eq!(
        run_web_source(
            r#"
class Mixed { get value() { return 2; } set get(f: any) {} }
class Later { get enumerable(): any { throw new Error("later"); } }
let msg = "none";
try { Object.defineProperties({}, { a: new Mixed() as any, b: new Later() as any }); } catch (e: any) { msg = e.name; }
console.log(msg);
"#
        ),
        "TypeError"
    );
}
