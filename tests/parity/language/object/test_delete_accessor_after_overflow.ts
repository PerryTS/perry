// Deleting an accessor (a class-body getter or setter, an object-literal getter) from an object that already holds
// more keys than its layout (after any delete, or many added keys): `in` must turn false, like hasOwnProperty, the
// read and the key list. Perry's `in` answered from the layout's accessor entry and kept saying true.
// Forms: computed and fixed key; getter, setter, method and data controls; class prototype and object literal;
// own `in`, inherited `in` through an instance, hasOwnProperty, Object.hasOwn, read, getOwnPropertyNames.
// Controls: a live accessor on an object whose own contents changed without an overflow (a declared method replaced
// in place) stays present.
const out: string[] = [];
function probe(tag: string, o: any, k: string, inst?: any): void {
  out.push(tag + " in=" + (k in o) + " own=" + o.hasOwnProperty(k) + " hasOwn=" + Object.hasOwn(o, k) +
    " read=" + typeof o[k] + (inst ? " inst-in=" + (k in inst) : "") +
    " names=" + Object.getOwnPropertyNames(o).filter((n) => n[0] !== "x").join("|"));
}
class A { m1() { return 1; } get g() { return 2; } set s(v: number) {} }
(A.prototype as any).x0 = 0;
delete (A.prototype as any).x0;
out.push("A-g " + delete (A.prototype as any)["g" + ""]); probe("A-g", A.prototype, "g", new A());
out.push("A-s " + delete (A.prototype as any)["s" + ""]); probe("A-s", A.prototype, "s", new A());
out.push("A-m " + delete (A.prototype as any)["m" + 1]); probe("A-m", A.prototype, "m1", new A());
class B { get g() { return 2; } }
for (let i = 0; i < 12; i++) (B.prototype as any)["x" + i] = i;
delete (B.prototype as any).x0;
out.push("B-fixed " + delete (B.prototype as any).g); probe("B-fixed", B.prototype, "g", new B());
class D { get g() { return 2; } }
for (let i = 0; i < 12; i++) (D.prototype as any)["x" + i] = i;
out.push("D-no-prior-delete " + delete (D.prototype as any)["g" + ""]); probe("D", D.prototype, "g", new D());
const lit: any = { get g() { return 2; }, y: 5 };
lit.x0 = 1; delete lit.x0;
out.push("lit " + delete lit["g" + ""]); probe("lit", lit, "g");
const lit2: any = { get g() { return 2; }, y: 5 };
for (let i = 0; i < 12; i++) lit2["x" + i] = i;
out.push("lit-many " + delete lit2.g); probe("lit-many", lit2, "g");
const data: any = { g: 1, y: 5 };
data.x0 = 1; delete data.x0;
out.push("data " + delete data["g" + ""]); probe("data", data, "g");
const kept: any = { get g() { return 2; }, y: 5 };
kept.x0 = 1; delete kept.x0;
probe("kept", kept, "g");
const def: any = {};
Object.defineProperty(def, "g", { get() { return 1; }, configurable: true });
def.x0 = 1; delete def.x0;
out.push("define " + delete def["g" + ""]); probe("define", def, "g");
class E { m() { return 1; } get g() { return 2; } }
(E.prototype as any).m = function () { return 3; };
probe("E-replaced", E.prototype, "g", new E());
(E.prototype as any)["m" + ""] = function () { return 4; };
probe("E-replaced-computed", E.prototype, "g", new E());
for (const line of out) console.log(line);
