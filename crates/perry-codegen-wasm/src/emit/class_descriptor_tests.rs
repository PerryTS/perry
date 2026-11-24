//! Property descriptors of a class value and its prototype on the web target.

use super::run_web_source;

// A class value's `prototype` is read-only, declared and as an expression. Expected
// output is Node's.
#[test]
fn a_class_prototype_property_is_read_only() {
    assert_eq!(
        run_web_source(
            r#"
class C { m() {} }
const K: any = class { q() {} };
function show(c: any) { const d: any = Object.getOwnPropertyDescriptor(c, "prototype"); return [d.writable, d.enumerable, d.configurable, d.value.constructor === c].join(","); }
(K as any).prototype = {};
console.log(show(C) + " " + show(K) + " " + typeof K.prototype.q);
"#
        ),
        "false,false,false,true false,false,false,true function"
    );
}

// An instance accessor is a non-enumerable accessor property of the class prototype, and
// a subclass's own accessor hides its parent's whole. Expected output is Node's.
#[test]
fn instance_accessors_are_properties_of_the_class_prototype() {
    assert_eq!(
        run_web_source(
            r#"
class C { _v = 1; get x() { return this._v; } set x(v: number) { this._v = v; } get ro() { return 5; } set wo(v: number) { this._v = v; } }
class D extends C { get x() { return 100 + super.x; } }
function show(o: any, k: string) { const d: any = Object.getOwnPropertyDescriptor(o, k); return d === undefined ? "none" : [typeof d.get, typeof d.set, d.enumerable, d.configurable, "value" in d].join(","); }
console.log([show(C.prototype, "x"), show(C.prototype, "ro"), show(C.prototype, "wo"), show(D.prototype, "x"), show(D.prototype, "ro")].join(" "));
const d: any = Object.getOwnPropertyDescriptor(C.prototype, "x");
const c: any = new C();
d.set.call(c, 9);
const e: any = new D();
e.x = 3;
const key = "dyn";
class K { get [key]() { return 1; } static get [key]() { return 2; } }
console.log([d.get.call(c), c.x, d.get.name, d.set.name, "x" in c, Object.keys(c), e.x, show(K.prototype, key), show(K, key), (new K() as any).dyn].join(" "));
"#
        ),
        "function,function,false,true,false function,undefined,false,true,false undefined,function,false,true,false function,undefined,false,true,false none\n\
         9 9 get x set x true _v 101 function,undefined,false,true,false function,undefined,false,true,false 1"
    );
}

// An instance reads what its prototype has now: a redefined or deleted accessor, an own
// field over an inherited accessor, or a throwing getter's error. Expected output is Node's.
#[test]
fn an_instance_read_follows_the_prototype_property() {
    assert_eq!(
        run_web_source(
            r#"
class R { get x() { return "R"; } }
Object.defineProperty(R.prototype, "x", { get: () => "redefined" });
class L { get m() { return 1; } }
delete (L.prototype as any).m;
class A { get x(): any { return "A"; } }
class B extends A { x = "own"; }
class T { get x() { throw new Error("boom"); } }
class E { get 0() { return "getter"; } get value() { return 1; } }
class S extends E { set value(v: number) {} }
const own: any = new E();
Object.defineProperty(own, "0", { value: "own0" });
let caught = "";
try { (new T() as any).x; } catch (e: any) { caught = e.message; }
async function inAsync() { const a: any = new A(); return a.x; }
console.log([new R().x, String((new L() as any).m), new B().x, Object.keys(new B()), own[0], String(new S().value), caught].join(" "));
inAsync().then((v) => console.log("async " + v));
"#
        ),
        "redefined undefined own x own0 undefined boom\nasync A"
    );
}

// A static accessor is a non-enumerable own property of the class value that runs with
// the class it is reached through as `this`, apart from an instance accessor of that
// name. Expected output is Node's.
#[test]
fn static_accessors_are_properties_of_the_class_value() {
    assert_eq!(
        run_web_source(
            r#"
class C { static _y = 1; static get y() { return this._y; } static set y(v: number) { this._y = v; } get y() { return "instance"; } static get name() { return "named"; } }
class E extends C { static get z() { return super.y + 1; } static get arrow() { return (() => super.hi())(); } static hi() { return "E"; } }
(C as any).hi = function () { return "C"; };
const d: any = Object.getOwnPropertyDescriptor(C, "y");
const K: any = C, S: any = E;
console.log([typeof d.get, typeof d.set, d.enumerable, d.configurable, Object.getOwnPropertyDescriptor(E, "y") === undefined, Object.keys(C)].join(","));
C.y = 4;
console.log([C.y, K.y, C._y, new C().y, C.name].join(","));
S.y = 7;
console.log([C.y, S.y, S.z, S.arrow, Object.keys(E)].join(","));
"#
        ),
        "function,function,false,true,true,_y,hi\n4,4,4,instance,named\n4,7,8,C,_y"
    );
}

// Each evaluation of a class in a function body has accessors that read its own
// captures. Expected output is Node's.
#[test]
fn accessors_of_a_class_evaluation_read_its_captures() {
    assert_eq!(
        run_web_source(
            r#"
function make(n: number): any { return class { static get s() { return n; } get i() { return n * 2; } }; }
const A = make(1), B = make(2);
const d: any = Object.getOwnPropertyDescriptor(A.prototype, "i");
console.log([A.s, B.s, new A().i, new B().i, typeof d.get, d.enumerable, d.get.call(new B())].join(","));
"#
        ),
        "1,2,2,4,function,false,2"
    );
}

// A static field redefined, made read-only or deleted through the class value reads and
// stores as that property. Expected output is Node's.
#[test]
fn a_redefined_static_field_reads_and_stores_as_redefined() {
    assert_eq!(
        run_web_source(
            r#"
class C { static x: any = 1; static z: any = 2; static r: any = 3; static d: any = 4; }
const K: any = C;
let stored: any;
Object.defineProperty(C, "x", { set: (v: any) => { stored = v; } });
Object.defineProperty(C, "z", { get: () => 42 });
Object.defineProperty(C, "r", { value: 7, writable: false, enumerable: false });
delete K.d;
C.x = 5;
C.r = 8;
console.log([String(C.x), String(K.x), stored, C.z, K.z, C.r, K.r, String(C.d), "d" in C, Object.keys(C)].join(","));
C.d = 6;
console.log([C.d, K.d, Object.keys(C)].join(","));
"#
        ),
        "undefined,undefined,5,42,42,7,7,undefined,false,x,z\n6,6,x,z,d"
    );
}

// A computed accessor belongs to the evaluation that defines it and reaches a subclass,
// and an evaluation inherits from its parent's prototype. Expected output is Node's.
#[test]
fn a_class_evaluation_owns_only_its_own_members() {
    assert_eq!(
        run_web_source(
            r#"
class Base { get x() { return "base"; } m() { return "m"; } }
function make(k: string): any { return class extends Base { get [k]() { return k; } }; }
const A = make("a"), B = make("b");
const own = (c: any, k: string) => Object.getOwnPropertyDescriptor(c.prototype, k) !== undefined;
const before = [own(A, "a"), own(A, "b"), own(B, "a"), own(B, "b"), own(B, "x"), own(B, "m"), new B().b, new B().x, new B().m()];
Object.defineProperty(Base.prototype, "x", { get: () => "redefined" });
const key = "key";
class P { get [key]() { return "computed"; } }
function sub(n: number): any { return class extends P { get n() { return n; } }; }
const q = new (sub(3))();
console.log(before.join(",") + " " + new B().x + " " + q.key + q.n);
"#
        ),
        "true,false,false,true,false,false,b,base,m redefined computed3"
    );
}

// A method named like an accessor's table entry stays a method, and a computed `#x`
// key is a public property. Expected output is Node's.
#[test]
fn accessor_keys_do_not_collide_with_method_names() {
    assert_eq!(
        run_web_source(
            r##"
const hash = "#label";
class C { __get_status() { return 1; } get status() { return 2; } static __get_s() { return 3; } static get s() { return 4; } get [hash]() { return 5; } static get [hash]() { return 6; } }
const c: any = new C(), K: any = C;
console.log([typeof Object.getOwnPropertyDescriptor(C.prototype, "__get_status")!.value, c.__get_status(), c.status, K.__get_s(), K.s, c[hash], K[hash]].join(","));
"##
        ),
        "function,1,2,3,4,5,6"
    );
}

// `super` in a static accessor reads the class value's current prototype. Expected
// output is Node's.
#[test]
fn static_super_follows_the_class_prototype() {
    assert_eq!(
        run_web_source(
            r#"
class A { static get x() { return "A" + (this as any).tag; } }
class B { static get x() { return "B" + (this as any).tag; } }
class D extends A { static tag = "d"; static get y() { return super.x; } }
const before = (D as any).y;
Object.setPrototypeOf(D, B);
console.log(before, (D as any).y);
"#
        ),
        "Ad Bd"
    );
}
