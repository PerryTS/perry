// Computed Number keys that name ordinary properties: a non-index key on an
// Array (arr[arr.length - 1] on an empty array reads "-1") and an integral
// key on an ordinary object (record[id]). Each read must see own, inherited,
// accessor and absent properties exactly as the string key does.
function at(o: any, k: number): any { return o[k]; }
const empty: any[] = [];
console.log("empty last", at(empty, empty.length - 1), at([7], 0), at([7, 8], -1));
const withNeg: any = [1, 2];
withNeg[-1] = "neg";
console.log("own neg", at(withNeg, -1), withNeg["-1"], at(withNeg, -2));
delete withNeg[-1];
console.log("deleted neg", at(withNeg, -1));
const big: any = [];
big[4294967295] = "not-an-index";
console.log("2^32-1", at(big, 4294967295), big.length, at(big, 2 ** 53), at(big, -(2 ** 53)));
console.log("fraction", at([1, 2], 1.5), at([1, 2], -0));
(Array.prototype as any)[-3] = "from Array.prototype";
(Object.prototype as any)[-4] = "from Object.prototype";
console.log("inherited", at([], -3), at([], -4), at({}, -4), at({}, -3));
Object.defineProperty(Array.prototype, "-5", { get() { return "getter len " + this.length; }, configurable: true });
console.log("getter", at([1, 2, 3], -5), at([], -5));
delete (Array.prototype as any)[-3];
delete (Object.prototype as any)[-4];
delete (Array.prototype as any)[-5];
console.log("after delete", at([], -3), at([], -4), at([], -5));
const custom: any = [];
Object.setPrototypeOf(custom, { "-1": "custom proto", 3: "proto index" });
console.log("custom proto", at(custom, -1), at(custom, 3), at(custom, 4));
// ordinary objects
const rec: any = { 1: "one", 22: "twenty-two", name: "r" };
console.log("record", at(rec, 1), at(rec, 22), at(rec, 3), at(rec, -1), at(rec, 2 ** 40));
rec[-7] = "minus seven";
rec[2 ** 40] = "big";
console.log("record more", at(rec, -7), at(rec, 2 ** 40), Object.keys(rec).join(","));
const acc: any = { get 5() { return "acc five"; } };
console.log("accessor", at(acc, 5));
const child: any = Object.create({ 9: "proto nine", [-9]: "proto minus nine" });
child[10] = "own ten";
console.log("chain", at(child, 9), at(child, -9), at(child, 10), at(child, 11));
(Object.prototype as any)[12] = "op twelve";
console.log("object proto index", at({}, 12), at(child, 12), at([], 12));
delete (Object.prototype as any)[12];
console.log("object proto index gone", at({}, 12), at([], 12));
const dyn: any = {};
for (let i = 0; i < 40; i++) dyn[i * 3] = i;
let s = 0;
for (let i = 0; i < 120; i++) { const v = at(dyn, i); if (v !== undefined) s += v; }
console.log("dense dyn", s, at(dyn, 117), at(dyn, 118));
class Sub extends Array<any> {}
const sub: any = new Sub();
sub.push("s0", "s1");
sub[-1] = "sub neg";
console.log("subclass", at(sub, 0), at(sub, 1), at(sub, 2), at(sub, -1), sub.length);
const frozen: any = Object.freeze({ 4: "four" });
console.log("frozen", at(frozen, 4), at(frozen, 5));
const p: any = new Proxy({}, { get(_t, k) { return "trap " + String(k); } });
console.log("proxy", at(p, -1), at(p, 3));
const proxyProto: any = Object.create(new Proxy({}, { get(_t, k) { return "proto trap " + String(k); } }));
console.log("proxy proto", at(proxyProto, -2), at(proxyProto, 6));
let last: any;
const obs: any[] = [];
for (let i = 0; i < 1000; i++) { if (i % 3 === 0) obs.push(i); else if (obs.length) obs.pop(); last = obs[obs.length - 1]; }
console.log("loop", last, obs.length);
