// Popping from an Array subclass retires that receiver's packed-numeric proof
// on the first pop and nothing afterwards. The flush of the process-global
// store-plan cache is now conditional on actually retiring one, so this pins
// that stores through those plans still see every property change.

class NumList extends Array<number> {
  tag: string;
  constructor(tag: string) {
    super();
    this.tag = tag;
  }
}

class Holder {
  a: number;
  b: number;
  c: number;
  constructor() {
    this.a = 0;
    this.b = 0;
    this.c = 0;
  }
}

const list = new NumList("l1");
for (let i = 0; i < 64; i++) list.push(i);
const h = new Holder();

// Interleave pops with stores through the same plans.
let acc = 0;
for (let i = 0; i < 32; i++) {
  const v = list.pop() as number;
  h.a = v;
  h.b = v * 2;
  h.c = h.a + h.b;
  acc += h.c;
}
console.log("interleaved", acc, list.length, h.a, h.b, h.c, list.tag);

// A property added to the prototype mid-loop MUST be seen by later stores:
// this is what the flush protects, so it must still work.
const proto = Object.getPrototypeOf(h) as any;
let setterSeen = 0;
Object.defineProperty(proto, "d", {
  set(v: number) {
    setterSeen += v;
  },
  get() {
    return setterSeen;
  },
  configurable: true,
});
for (let i = 0; i < 8; i++) {
  list.pop();
  (h as any).d = i;
}
console.log("setter-after-pop", setterSeen, (h as any).d, list.length);

// Freezing after pops is honored too (non-strict: silent no-op).
const h2 = new Holder();
h2.a = 5;
list.pop();
Object.freeze(h2);
try {
  h2.a = 99;
} catch (e) {
  console.log("threw", (e as Error).constructor.name);
}
console.log("frozen", h2.a, Object.isFrozen(h2));

// Mixed element kinds retire the numeric proof; pops must still be correct.
const mixed = new NumList("mixed");
mixed.push(1);
mixed.push(2);
(mixed as any).push("three");
mixed.push(4);
const popped: unknown[] = [];
for (let i = 0; i < 4; i++) popped.push(mixed.pop());
console.log("mixed", JSON.stringify(popped), mixed.length, mixed.tag);

// Subclass identity survives the whole sequence.
console.log(
  "identity",
  list instanceof NumList,
  list instanceof Array,
  Array.isArray(list),
  list.length,
);
