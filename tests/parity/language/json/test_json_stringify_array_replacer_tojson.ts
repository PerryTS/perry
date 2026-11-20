// JSON.stringify with an array replacer (PropertyList) still runs
// SerializeJSONProperty's toJSON step for every value: the root, each
// whitelisted member and each array element, with the property key.
class WithOwn {
  a: number;
  b: number;
  toJSON: (k: string) => unknown;
  constructor() {
    this.a = 1;
    this.b = 2;
    this.toJSON = (k: string) => ({ a: "own:" + k, b: 9, c: 3 });
  }
}
class WithProto {
  a = 10;
  b = 20;
  toJSON(k: string) {
    return { a: "proto:" + k, z: 1 };
  }
}
class Sub extends WithProto {
  b = 30;
}
class Prim {
  a = 5;
  toJSON(k: string) {
    return k === "" ? 42 : "prim:" + k;
  }
}

// 1. instance (own) toJSON at the root: called with "", result filtered.
console.log(JSON.stringify(new WithOwn(), ["a", "c"]));
// 2. prototype toJSON at the root and from a parent class.
console.log(JSON.stringify(new WithProto(), ["a", "z"]));
console.log(JSON.stringify(new Sub(), ["a", "b"]));
// 3. nested: a member whose value has toJSON gets its key.
console.log(JSON.stringify({ a: new WithProto(), b: 1, x: new WithOwn() }, ["a", "b", "x"]));
// 4. toJSON returning a primitive (root number, member string).
console.log(JSON.stringify(new Prim(), ["a"]));
console.log(JSON.stringify({ a: new Prim(), b: [new Prim()] }, ["a", "b"]));
// 5. Date under an array replacer (Date.prototype.toJSON).
const d = new Date(Date.UTC(2020, 0, 2, 3, 4, 5, 6));
console.log(JSON.stringify({ a: d, b: [d] }, ["a", "b"]));
console.log(JSON.stringify(d, ["a"]));
// 6. array elements get their index as the key.
console.log(JSON.stringify([new WithProto(), { a: new Prim() }], ["a"]));
// 7. duplicate and numeric keys in the replacer array.
const n = { 1: new Prim(), a: new WithProto(), "2": 7 };
console.log(JSON.stringify(n, ["a", 1, "a", 2, 1]));
// 8. toJSON result that is an object whose member has toJSON: member's runs, the result's own does not re-run.
const outer = {
  toJSON() {
    return { a: new Prim(), toJSON: "not called again" };
  },
};
console.log(JSON.stringify(outer, ["a", "toJSON"]));
// 9. pretty form keeps the same values.
console.log(JSON.stringify({ a: new WithProto(), b: d }, ["a", "b"], 2));
// 10. a toJSON that returns undefined drops the member.
console.log(JSON.stringify({ a: { toJSON: () => undefined }, b: 1 }, ["a", "b"]));
// 11. a root toJSON with no JSON text makes stringify return undefined.
console.log(JSON.stringify({ toJSON: () => undefined }, ["a"]));
console.log(JSON.stringify({ toJSON: () => Symbol("s") }, ["a"]));
// 12. array elements whose toJSON has no JSON text become null; a symbol member is left out.
console.log(JSON.stringify([{ toJSON: () => undefined }, { toJSON: () => () => 1 }, 3], ["a"]));
console.log(JSON.stringify({ a: { toJSON: () => Symbol("t") }, b: 2 }, ["a", "b"]));
// 13. a getter in the list runs and its value's toJSON gets the key.
const g = {
  get a() {
    return new Prim();
  },
  b: 1,
};
console.log(JSON.stringify(g, ["a", "b"]));
