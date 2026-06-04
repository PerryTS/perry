function cell(value: any): string {
  return typeof value + ":" + String(value);
}

function list(values: any): string {
  const out = [];
  for (let i = 0; i < values.length; i++) {
    out.push(cell(values[i]));
  }
  return out.join("|");
}

function attempt(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (err: any) {
    console.log(label, err && err.name);
  }
}

const signed = new BigInt64Array(3);
signed[0] = 5n;
signed[1] = -1n;
signed[2] = 9223372036854775808n;
console.log("signed direct", list(signed));
console.log("signed accessors", cell(signed[1]), cell(signed["2" as any]), cell(signed.at(0)));

const unsigned = new BigUint64Array(3);
unsigned[0] = 5n;
unsigned[1] = -1n;
unsigned[2] = 18446744073709551616n;
console.log("unsigned direct", list(unsigned));

attempt("signed number write", () => {
  (signed as any)[0] = 1;
});
attempt("unsigned number write", () => {
  (unsigned as any)[0] = 1;
});
console.log("after direct rejects", cell(signed[0]), cell(unsigned[0]));

Object.defineProperty(signed, "0", { value: 42n });
Object.defineProperty(unsigned, "1", { value: -2n });
const desc = Object.getOwnPropertyDescriptor(signed, "0") as any;
console.log(
  "define values",
  cell(signed[0]),
  cell(unsigned[1]),
  cell(desc.value),
  desc.writable,
  desc.enumerable,
  desc.configurable,
);
attempt("define number value", () => {
  Object.defineProperty(signed, "1", { value: 1 });
});
console.log("after define reject", cell(signed[1]));

const fromArray = new BigInt64Array([1n, -2n, "3" as any, true as any]);
console.log("from array", list(fromArray));
attempt("from number array", () => {
  new BigInt64Array([1 as any]);
});

const setTarget = new BigInt64Array(4);
setTarget.set([3n, -4n]);
setTarget.set(new BigInt64Array([5n, 6n]), 2);
console.log("set values", list(setTarget));
attempt("set number source", () => {
  setTarget.set([1 as any], 1);
});
console.log("after set reject", list(setTarget));

setTarget.fill(7n, 1, 3);
console.log("fill bigint", list(setTarget));
attempt("fill number", () => {
  setTarget.fill(1 as any);
});
console.log("after fill reject", list(setTarget));

const copiedUnsigned = new BigUint64Array(unsigned);
console.log("copy unsigned", list(copiedUnsigned));
