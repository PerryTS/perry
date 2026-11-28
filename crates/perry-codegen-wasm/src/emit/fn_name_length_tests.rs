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

// A constructor's hidden capture parameters are not part of its `length`, and a method
// with a symbol key is named by the symbol's bracketed description. Output is Node's.
#[test]
fn captures_and_symbol_keys_are_not_part_of_name_and_length() {
    assert_eq!(
        run_web_source(
            r#"
const work = Symbol("work");
const bare = Symbol();
class K { [work](a: number) { return a; } [bare]() {} }
function local(cap: number) {
  class A { z: number; constructor(a: number) { this.z = a + cap; } m(x: number, y: number) { return x + cap + y; } }
  class B { m() { return cap; } }
  class D { z: number; constructor(a: number, b = cap) { this.z = a + b + cap; } }
  console.log(A.length, new A(1).m.length, B.length, D.length);
}
local(1);
const k: any = new K();
console.log(k[work].name, k[bare].name === "", typeof k[work].name);
"#
        ),
        "1 2 0 1\n[work] true string"
    );
}

// A computed method's `length`, instance and static, a function that reads `arguments`,
// and an anonymous arrow returned from a function. Expected output is Node's.
#[test]
fn computed_methods_and_arguments_have_their_length() {
    assert_eq!(
        run_web_source(
            r#"
const work = Symbol("work");
class K {
  [work](a: number, b: number) { return a + b; }
  ["str"](a: number) { return a; }
  static [work](a: number, b: number, c: number) { return a; }
}
function withArgs(a: number, b: number) { return arguments.length + a + b; }
function mk() { return (x: number) => x; }
const k: any = new K();
const S: any = K;
console.log(k[work].name, k[work].length, k.str.name, k.str.length, S[work].name, S[work].length);
console.log(withArgs.length, mk().name === "", mk().length);
"#
        ),
        "[work] 2 str 1 [work] 3\n2 true 1"
    );
}
