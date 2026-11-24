//! A function's `name` and `length` on the web target.

use super::run_web_source;

// Declarations, arrows, function expressions, async functions, object-literal members,
// class constructors and methods carry Node's `name` and `length` (parameters before the
// first default or rest), in a synchronous and an async body. Expected output is Node's.
#[test]
fn functions_have_their_name_and_length() {
    assert_eq!(
        run_web_source(
            r#"
function decl(a: number, b: number): number { return a + b; }
const arrow = (x: number, y: number, z: number): number => x + y + z;
async function adecl(a: number): Promise<number> { return a; }
const fexpr = function (k: number): number { return k; };
const named = function inner(k: number, j: number): number { return k + j; };
class C {
  constructor(a: number, b: number) {}
  m(x: number): number { return x; }
  static s(a: number, b: number, c: number): number { return a; }
}
class D extends C {}
function opt(a: number, b: number = 2, ...rest: number[]): number { return a + b; }
const o = { meth(p: number, q: number) { return p; }, prop: (r: number) => r };
const c = new C(1, 2);
console.log(decl.name, decl.length, arrow.name, arrow.length, adecl.name, adecl.length);
console.log(fexpr.name, fexpr.length, named.name, named.length, opt.name, opt.length);
console.log(C.name, C.length, D.name, D.length, c.m.name, c.m.length, C.s.name, C.s.length);
console.log(o.meth.name, o.meth.length, o.prop.name, o.prop.length, ((a: number) => a).name === "");
async function inAsync(): Promise<void> {
  const local = (p: number, q: number) => p;
  console.log("async", decl.name, decl.length, local.name, local.length);
}
inAsync();
"#
        ),
        "decl 2 arrow 3 adecl 1\nfexpr 1 inner 2 opt 1\nC 2 D 0 m 1 s 3\nmeth 2 prop 1 true\nasync decl 2 local 2"
    );
}
