// A function's [[Prototype]] is its shape's identity while its shape describes
// it, and its own record's once it is a dictionary; reads, getPrototypeOf and
// instanceof all follow the one link.
const P = Object.getPrototypeOf;
const proto = { inherited: "from-proto", get viaThis() { return typeof this === "function" ? "fn:" + (this as any).tag : "other"; } };
function plain() { return 1; }
(plain as any).tag = "plain";
Object.setPrototypeOf(plain, proto);
console.log("1", P(plain) === proto, (plain as any).inherited, (plain as any).viaThis, typeof (plain as any).call, plain());
const bound: any = Object.assign(Object.setPrototypeOf(Function.prototype.bind.call(plain, null), proto), { tag: "bound" });
console.log("2", P(bound) === proto, bound.inherited, bound.viaThis, bound.name, bound.length, typeof bound.apply);
async function af() { return 2; }
Object.setPrototypeOf(af, proto);
console.log("3", P(af) === proto, (af as any).inherited, typeof (af as any).then);
class Base { m() { return "Base.m"; } get g() { return "g:" + (this as any).tag; } }
class Derived extends Base {}
const callable: any = Object.assign(Object.setPrototypeOf(((x: number) => x * 2).bind(undefined), Derived.prototype), { tag: "callable" });
console.log("4", P(callable) === Derived.prototype, callable.m(), callable.g, callable instanceof Derived, callable(21));
const nul: any = Object.setPrototypeOf(function n() {}, null);
console.log("5", P(nul), typeof nul.call, typeof nul.toString, nul.name, "call" in nul);
// relink twice, then back to Function.prototype
const re: any = function re() {};
Object.setPrototypeOf(re, proto);
Object.setPrototypeOf(re, { other: "second" });
console.log("6", re.other, re.inherited, P(re) === proto);
Object.setPrototypeOf(re, Function.prototype);
console.log("7", P(re) === Function.prototype, re.other, typeof re.call);
// an accessor and a delete after a link: the link stays
const dict: any = function dict() {};
Object.setPrototypeOf(dict, proto);
Object.defineProperty(dict, "acc", { get() { return "own-acc"; }, configurable: true });
console.log("8", P(dict) === proto, dict.acc, dict.inherited);
delete dict.name;
console.log("9", P(dict) === proto, dict.name, dict.inherited);
Object.setPrototypeOf(dict, { fresh: "fresh" });
console.log("10", dict.fresh, dict.inherited, dict.acc);
// own keys shadow the link
const sh: any = Object.setPrototypeOf(function sh() {}, proto);
sh.inherited = "own";
console.log("11", sh.inherited, P(sh) === proto);
// a custom @@hasInstance on the linked chain
const hp = { [Symbol.hasInstance](v: any) { return v === 42; } };
const hf: any = Object.setPrototypeOf(function hf() {}, hp);
console.log("12", (42 as any) instanceof hf, ({} as any) instanceof hf);
// Function.prototype is not on a relinked function's chain
(Function.prototype as any).fpx = "fp";
console.log("13", (plain as any).fpx, (function () {} as any).fpx, bound.fpx);
delete (Function.prototype as any).fpx;
// a function linked off %Function.prototype% is an instance of Function only
// through its link, and inherits no `constructor` from a null link
console.log("14", callable instanceof Function, plain instanceof Function, nul instanceof Function, typeof nul.constructor, (function () {}) instanceof Function);
