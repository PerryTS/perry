// Object.assign from a plain source whose shape answers CopyDataProperties
// (positional copy) stores through strict [[Set]]: own overwrites, adds on
// targets with custom prototypes, and every case the preflight must refuse.

const proto: any = { _tag: "Some", shared: 1 };

// Warm adds on Object.create(proto) targets: same shape every iteration.
let sum = 0;
const held: any[] = [];
for (let i = 0; i < 2000; i++) {
  const c = Object.assign(Object.create(proto), { value: i, extra: i & 1 });
  held.push(c);
  sum += c.value + c.extra;
}
console.log(sum, held.length, Object.keys(held[7]), held[7]._tag);
console.log(Object.getPrototypeOf(held[3]) === proto, proto.value, "value" in proto);

// Inherited writable data: Set creates an own property, the prototype keeps its value.
const shadow = Object.assign(Object.create(proto), { shared: 2 });
console.log(shadow.shared, proto.shared, Object.keys(shadow));

// Overwrite of existing own keys keeps their order; new keys append.
const target: any = { b: 1, a: 2 };
Object.assign(target, { a: 3, c: 4, b: 5 });
console.log(JSON.stringify(target));

// Number and object values through the same store, many keys (spill).
const big: any = {};
for (let k = 0; k < 24; k++) big["k" + k] = k % 3 === 0 ? { n: k } : k * 1.5;
const copy = Object.assign(Object.create(proto), big);
console.log(Object.keys(copy).length, copy.k3.n, copy.k4, copy.k23);
const copy2 = Object.assign(Object.create(proto), big);
console.log(JSON.stringify(Object.keys(copy2)) === JSON.stringify(Object.keys(big)));

// Inherited setter: Set runs it with the target as receiver, no own property.
const log: string[] = [];
const withSetter: any = {
  set value(v: any) {
    log.push("set " + v);
  },
};
const t2 = Object.assign(Object.create(withSetter), { value: 9, other: 1 });
console.log(log.join(","), Object.keys(t2), Object.getOwnPropertyNames(t2));

// Inherited read-only data: strict Set throws.
const ro = Object.defineProperty({}, "value", { value: 1, writable: false });
try {
  Object.assign(Object.create(ro), { value: 2 });
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}

// Own read-only and frozen targets throw; non-extensible refuses adds.
try {
  Object.assign(Object.freeze({ value: 1 }), { value: 2 });
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}
try {
  Object.assign(Object.preventExtensions({ a: 1 }), { a: 2, b: 3 });
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}

// Prototype changed between assigns: the new chain is consulted.
const p2: any = { value: 0 };
const t3: any = Object.create(p2);
Object.assign(t3, { value: 1 });
Object.defineProperty(p2, "late", {
  set(v: any) {
    log.push("late " + v);
  },
});
const t4: any = Object.create(p2);
Object.assign(t4, { late: 5, value: 2 });
console.log(t3.value, t4.value, Object.keys(t4), log.join(","));

// Integer-like and __proto__ source keys keep ordinary semantics.
const t5 = Object.assign(Object.create(proto), { b: 1, 2: "two", 1: "one" });
console.log(JSON.stringify(Object.keys(t5)), t5[1], t5[2]);
const src6 = JSON.parse('{"__proto__": {"x": 1}, "y": 2}');
const t6: any = Object.assign({}, src6);
console.log(Object.keys(t6), t6.x, t6.y);

// Null-prototype targets.
const t7 = Object.assign(Object.create(null), { value: 1, extra: 2 });
console.log(Object.keys(t7), t7.value + t7.extra);

// process.env receives assigned keys through its own setter.
Object.assign(process.env, { PERRY_GAP_ASSIGN_POSITIONAL: "yes" });
console.log(process.env.PERRY_GAP_ASSIGN_POSITIONAL);
