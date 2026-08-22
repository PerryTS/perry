const k = Symbol("k");
const proto: any = {
  get method() { return (this as any)[k] ? (this as any)[k].m : "GET"; },
  get plusOne() { return (this as any).x + 1; },
};
const inst: any = Object.create(proto);
inst[k] = { m: "POST" };
inst.x = 41;
if (inst.method !== "POST" || inst.plusOne !== 42) throw new Error("instance getter this");

const inst2: any = Object.create(proto);
inst2[k] = { m: "PUT" };
inst2.x = 9;
if (inst2.method !== "PUT" || inst2.plusOne !== 10) throw new Error("second instance getter this");

const own: any = { _v: 5, get v() { return (this as any)._v; } };
if (own.v !== 5) throw new Error("own accessor this");

const sproto: any = { set val(v: number) { (this as any)._v = v * 2; }, get val() { return (this as any)._v; } };
const sinst: any = Object.create(sproto);
sinst.val = 21;
if (sinst.val !== 42) throw new Error("inherited setter this");

class A { v = 0; get x() { return (this as any).v + 100; } }
const leaf: any = Object.create(new A());
leaf.v = 5;
if (leaf.x !== 105) throw new Error("inherited class getter this");
console.log("OK");
