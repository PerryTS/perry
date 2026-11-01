// Each fast path the one-authority [[Prototype]] model takes must still
// answer what the full operation does.
const log: string[] = [];
// A store on an object whose chain holds a setter runs the setter; on a null
// chain it creates the own property.
const withSetter = { set zz(_v: number) { log.push("setter"); } };
const o: any = Object.setPrototypeOf({}, withSetter);
o.zz = 1;
const n: any = Object.create(null);
n.zz = 1;
console.log("1", Object.prototype.hasOwnProperty.call(o, "zz"), log.join(","), n.zz, Object.keys(n).join(","));
// A relinked chain's toJSON is found; a null chain has none.
const tj = Object.setPrototypeOf({ a: 1 }, { toJSON() { return "TJ"; } });
const nj = Object.assign(Object.create(null), { a: 2 });
console.log("2", JSON.stringify(tj), JSON.stringify(nj), JSON.stringify([tj, nj]));
// Every body kind stands on its own intrinsic until a link moves it.
async function af() {}
function* gf() {}
const AsyncFunction = Object.getPrototypeOf(af);
console.log("3", Object.getPrototypeOf(function () {}) === Function.prototype, Object.getPrototypeOf(af) === AsyncFunction,
  Object.getPrototypeOf(gf) !== Function.prototype, typeof (function () {}).call, (() => 1) instanceof Function);
// A key added after a link keeps the link.
const p = { inherited: "p" };
const fk: any = Object.setPrototypeOf(function fk() {}, p);
fk.k1 = 1; fk.k2 = 2;
console.log("4", Object.getPrototypeOf(fk) === p, fk.inherited, fk.k1 + fk.k2, Object.keys(fk).join(","));
// A bound function linked to a chain with a setter runs it; a readonly key
// on the chain blocks the store.
const ro = Object.defineProperty({}, "fixed", { value: 1, writable: false });
const b: any = Object.setPrototypeOf(function t() {}.bind(null), Object.setPrototypeOf({ set zz(_v: number) { log.push("bound-setter"); } }, ro));
b.zz = 1;
const fixedSet = Reflect.set(b, "fixed", 2);
b.free = 3;
console.log("5", Object.prototype.hasOwnProperty.call(b, "zz"), log.join(","), fixedSet, b.fixed, b.free);
// A plain function stands on %Function.prototype%, not another body kind's
// intrinsic, with or without own keys.
const AF: any = Object.getPrototypeOf(af);
AF.onAsync = "A";
const pk: any = function pk() {}; pk.own = 1;
const pn: any = function pn() {};
console.log("6", pk.onAsync, pn.onAsync, (af as any).onAsync);
delete AF.onAsync;
