// A function's name and length: declarations, arrows, function expressions, async
// functions, object-literal members, class constructors and methods, in a sync and an
// async body. length counts the parameters before the first default or rest.
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
