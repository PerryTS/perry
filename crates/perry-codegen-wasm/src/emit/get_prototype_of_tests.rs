//! `Object.getPrototypeOf` and `Reflect.getPrototypeOf` on the web target.

use super::run_web_source;

// `Object.getPrototypeOf` and `Reflect.getPrototypeOf` answer with the host's
// [[Prototype]], in a synchronous and in an async body. Expected output is Node's.
#[test]
fn get_prototype_of_answers_with_the_prototype() {
    assert_eq!(
        run_web_source(
            r#"
class A {}
const base = { k: 1 };
const child: any = { own: 2 };
Object.setPrototypeOf(child, base);
const bare: any = {};
Object.setPrototypeOf(bare, null);
function plain() {}
const arrow = () => 0;
function viaObject() { return [Object.getPrototypeOf({}) === Object.prototype, Object.getPrototypeOf([1]) === Array.prototype, Object.getPrototypeOf(child) === base, Object.getPrototypeOf(bare) === null, Object.getPrototypeOf(1) === Number.prototype, Object.getPrototypeOf(A) === Function.prototype, Object.getPrototypeOf(plain) === Function.prototype, Object.getPrototypeOf(arrow) === Function.prototype].join(","); }
function viaReflect() { let threw = "no"; try { Reflect.getPrototypeOf(1 as any); } catch (e: any) { threw = e.name; } return [Reflect.getPrototypeOf(child) === base, Reflect.getPrototypeOf(plain) === Function.prototype, Reflect.getPrototypeOf(arrow) === Function.prototype, threw].join(","); }
function nullish() { try { Object.getPrototypeOf(null); return "no throw"; } catch (e: any) { return e.name; } }
async function inAsync() { return (Object.getPrototypeOf(child) === base) + "," + (Reflect.getPrototypeOf([]) === Array.prototype); }
console.log(viaObject() + " | " + viaReflect() + " | " + nullish());
inAsync().then((v) => console.log(v));
"#
        ),
        "true,true,true,true,true,true,true,true | true,true,true,TypeError | TypeError\ntrue,true"
    );
}

// An async function, a generator and an async generator answer their own
// intrinsic prototype, declared and as an expression. Expected output is Node's.
#[test]
fn get_prototype_of_a_function_answers_its_kind() {
    assert_eq!(
        run_web_source(
            r#"
const AsyncProto = Object.getPrototypeOf(async () => {});
const GenProto = Object.getPrototypeOf(function* () {});
const AsyncGenProto = Object.getPrototypeOf(async function* () {});
async function a() {}
function* g() { yield 1; }
async function* ag() { yield 1; }
const ae = async function () {};
const ge = function* () { yield 1; };
const age = async function* () { yield 1; };
function plain() {}
(a as any).protoKind = 0;
(plain as any).protoKind = 99;
console.log([AsyncProto !== Function.prototype, GenProto !== Function.prototype, AsyncGenProto !== Function.prototype, AsyncProto !== GenProto, GenProto !== AsyncGenProto].join(","));
console.log([Object.getPrototypeOf(a) === AsyncProto, Object.getPrototypeOf(g) === GenProto, Object.getPrototypeOf(ag) === AsyncGenProto, Reflect.getPrototypeOf(a) === AsyncProto, Object.getPrototypeOf(plain) === Function.prototype].join(","));
console.log([Object.getPrototypeOf(ae) === AsyncProto, Object.getPrototypeOf(ge) === GenProto, Object.getPrototypeOf(age) === AsyncGenProto].join(","));
"#
        ),
        "true,true,true,true,true\ntrue,true,true,true,true\ntrue,true,true"
    );
}

// A class method read as a value, and a function passed to a host function
// (`gp(af)`, `Reflect.apply`), answer their kind's prototype. Expected output is Node's.
#[test]
fn get_prototype_of_a_method_value_or_host_argument_answers_its_kind() {
    assert_eq!(
        run_web_source(
            r#"
class C { *g() {} async a() {} async *ag() {} m() {} static *sg() {} static async sa() {} }
const AsyncProto = Object.getPrototypeOf(async () => {});
const GenProto = Object.getPrototypeOf(function* () {});
const AsyncGenProto = Object.getPrototypeOf(async function* () {});
const c = new C();
console.log([Object.getPrototypeOf(c.g) === GenProto, Object.getPrototypeOf(c.a) === AsyncProto, Object.getPrototypeOf(c.ag) === AsyncGenProto, Object.getPrototypeOf(c.m) === Function.prototype, Object.getPrototypeOf(C.sg) === GenProto, Object.getPrototypeOf(C.sa) === AsyncProto].join(","));
const gp = Object.getPrototypeOf;
const af = async () => {};
console.log([gp(af) === AsyncProto, Reflect.apply(gp, null, [af]) === AsyncProto, gp(c.g) === GenProto, Reflect.apply(gp, null, [function* () {}]) === GenProto].join(","));
const k = "cg";
class D { *[k]() {} static async [k]() {} }
console.log([Object.getPrototypeOf(new D().cg) === GenProto, Object.getPrototypeOf((D as any).cg) === AsyncProto].join(","));
(GenProto as any).apply = null;
console.log(typeof Reflect.apply(c.g, c, []));
"#
        ),
        "true,true,true,true,true,true\ntrue,true,true,true\ntrue,true\nobject"
    );
}
