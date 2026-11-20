function descriptorShape(obj: any, key: string) {
  const desc = Object.getOwnPropertyDescriptor(obj, key);
  if (!desc) {
    return null;
  }
  const out: Record<string, unknown> = {};
  if ("value" in desc) {
    out.value = desc.value;
    out.writable = desc.writable;
  }
  if ("get" in desc) {
    out.get = typeof desc.get;
    out.set = typeof desc.set;
  }
  out.enumerable = desc.enumerable;
  out.configurable = desc.configurable;
  return out;
}

function attempt(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name);
  }
}

function show(label: string, value: unknown) {
  console.log(label, JSON.stringify(value));
}

const ta: any = new Uint8Array([2, 3, 4]);

attempt("define length", () => {
  Object.defineProperty(ta, "length", { value: 1 });
});
show("own length", {
  read: ta.length,
  tail: ta[2],
  desc: descriptorShape(ta, "length"),
  names: Object.getOwnPropertyNames(ta),
  keys: Object.keys(ta),
  hasOwn: Object.prototype.hasOwnProperty.call(ta, "length"),
  enumerable: Object.prototype.propertyIsEnumerable.call(ta, "length"),
});

attempt("define foo", () => {
  Object.defineProperty(ta, "foo", { value: 5 });
});
ta.bar = 7;
show("ordinary props", {
  foo: ta.foo,
  fooDesc: descriptorShape(ta, "foo"),
  bar: ta.bar,
  barDesc: descriptorShape(ta, "bar"),
  names: Object.getOwnPropertyNames(ta),
  keys: Object.keys(ta),
});

let getterCalls = 0;
Object.defineProperty(ta, "acc", {
  get() {
    getterCalls++;
    return this.length + 10;
  },
  set(value: number) {
    this.seen = value;
  },
  enumerable: true,
  configurable: true,
});
const accRead = ta.acc;
ta.acc = 88;
show("accessor prop", {
  accRead,
  getterCalls,
  seen: ta.seen,
  accDesc: descriptorShape(ta, "acc"),
  seenDesc: descriptorShape(ta, "seen"),
  keys: Object.keys(ta),
});

const nonCanonical: any = new Uint8Array([1, 2]);
Object.defineProperty(nonCanonical, "00", {
  value: 9,
  writable: true,
  enumerable: true,
  configurable: true,
});
show("non canonical key", {
  value: nonCanonical["00"],
  names: Object.getOwnPropertyNames(nonCanonical),
  keys: Object.keys(nonCanonical),
});

const indexed: any = new Uint8Array([5, 6]);
attempt("define index", () => {
  Object.defineProperty(indexed, "0", { value: 9 });
});
indexed["1"] = 8;
show("index props", {
  zero: indexed[0],
  one: indexed[1],
  zeroDesc: descriptorShape(indexed, "0"),
  names: Object.getOwnPropertyNames(indexed),
  keys: Object.keys(indexed),
});
attempt("define oob index", () => {
  Object.defineProperty(indexed, "2", { value: 10 });
});
attempt("define negative index", () => {
  Object.defineProperty(indexed, "-1", { value: 10 });
});
show("invalid index state", {
  negative: indexed["-1"],
  negativeDesc: descriptorShape(indexed, "-1"),
  names: Object.getOwnPropertyNames(indexed),
});
