// Object-literal methods read the receiver of each call, whatever else the
// literal holds: a computed key, an accessor, a spread, a `__proto__` entry.
// Prototype literals of this kind are how Effect, fp-ts and most libraries
// build their data types (`Object.create(Proto)`), so a method inherited
// through such a prototype must see the instance as `this`.

const tag = Symbol("tag");
const base = { kind: "base", hello() { return "hello from " + (this as any).kind; } };

const withComputed: any = {
  [Symbol.iterator]() { return [(this as any).value][Symbol.iterator](); },
  [tag]() { return "tag:" + (this as any).value; },
  ["comp" + "uted"]() { return "computed:" + (this as any).value; },
  read() { return (this as any).value; },
};
const withAccessor: any = {
  get doubled() { return (this as any).value * 2; },
  set doubled(v: number) { (this as any).value = v / 2; },
  read() { return (this as any).value; },
};
const withSpread: any = {
  ...base,
  kind: "spread",
  read() { return (this as any).value; },
};
const withProto: any = {
  __proto__: base,
  kind: "proto",
  read() { return (this as any).value + ":" + (this as any).hello(); },
};

function make(proto: any, value: number) {
  const o = Object.create(proto);
  o.value = value;
  return o;
}

const a = make(withComputed, 1);
console.log("computed read", a.read(), [...a].join(","), a[tag](), a.computed());
const b = make(withAccessor, 2);
console.log("accessor read", b.read(), b.doubled);
b.doubled = 10;
console.log("accessor set", b.read(), Object.prototype.hasOwnProperty.call(withAccessor, "value"));
const c = make(withSpread, 3);
console.log("spread read", c.read(), c.hello());
const d = make(withProto, 4);
console.log("proto read", d.read());

// The literal itself as receiver, and an explicit receiver.
withComputed.value = "self";
console.log("self", withComputed.read(), withComputed[tag]());
console.log("call", withComputed.read.call({ value: "other" }), withAccessor.read.apply({ value: "applied" }));
const borrowed = { value: "borrowed", read: withSpread.read };
console.log("borrowed", borrowed.read());
const detached = withProto.read;
try {
  detached();
  console.log("detached no throw");
} catch (e) {
  console.log("detached throws", e instanceof TypeError);
}

// A method keeps its own receiver per call, also through a dispatch loop.
const many = [make(withComputed, 10), make(withComputed, 20), make(withComputed, 30)];
let sum = 0;
for (let i = 0; i < 3000; i++) sum += many[i % 3].read();
console.log("loop", sum);

// Arrow functions inside a method see the method's receiver.
const arrows: any = {
  [tag]: 1,
  value: 5,
  later() { return [1, 2].map((x) => x * (this as any).value); },
};
console.log("arrows", arrows.later().join(","), make(arrows, 7).later().join(","));

// `super` inside a method of a literal with a computed key.
const parent = { greet() { return "parent:" + (this as any).name; } };
const child: any = {
  __proto__: parent,
  [tag]: true,
  name: "child",
  greet() { return "child>" + super.greet(); },
};
console.log("super", child.greet(), make(child, 0).greet.call({ name: "x" }));

// Generator and async methods keep the call receiver too.
const gens: any = {
  [tag]: 0,
  *items() { yield (this as any).value; yield (this as any).value + 1; },
  async later() { return (this as any).value * 3; },
};
const g = make(gens, 40);
console.log("generator", [...g.items()].join(","));
g.later().then((v: number) => console.log("async", v));

// Names and own keys are the spec's.
console.log(
  "names",
  withComputed.read.name,
  withComputed[Symbol.iterator].name,
  withComputed[tag].name,
  withComputed.computed.name,
);
console.log("keys", Object.keys(withComputed).join(","), Object.keys(withAccessor).join(","));
console.log("desc", JSON.stringify(Object.getOwnPropertyDescriptor(withComputed, "read")?.enumerable));
