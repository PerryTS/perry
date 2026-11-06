// @ts-nocheck
function show(label, value) {
  console.log(label + ":" + String(value) + ":" + typeof value);
}

function showThrow(label, fn) {
  try {
    const value = fn();
    console.log(label + ":ok:" + String(value) + ":" + typeof value);
  } catch (err) {
    console.log(label + ":throw:" + err.name + ":" + err.message);
  }
}

const sab = new SharedArrayBuffer(24);
const i64 = new BigInt64Array(sab, 0, 2);
const u64 = new BigUint64Array(sab, 16, 1);
const numericSab = new SharedArrayBuffer(16);
const numeric = new Int32Array(numericSab, 0, 4);

show("load initial", Atomics.load(i64, 0));
show("store string", Atomics.store(i64, 0, "-3"));
show("add previous", Atomics.add(i64, 0, 10n));
show("load after add", Atomics.load(i64, 0));
show("sub previous", Atomics.sub(i64, 0, 4n));
show("exchange previous", Atomics.exchange(i64, 0, 0x7fffffffffffffffn));
show(
  "compareExchange hit",
  Atomics.compareExchange(i64, 0, 0x7fffffffffffffffn, -1n),
);
show(
  "compareExchange miss",
  Atomics.compareExchange(i64, 0, 0x7fffffffffffffffn, 4n),
);
show("and previous", Atomics.and(i64, 0, 0xffn));
show("or previous", Atomics.or(i64, 0, 0x10n));
show("xor previous", Atomics.xor(i64, 0, 0xffn));
show("final", Atomics.load(i64, 0));

show("i64 high or previous", Atomics.or(i64, 1, 0x100000000n));
show("i64 high or after", Atomics.load(i64, 1));

show("u64 store wrap", Atomics.store(u64, 0, -1n));
show("u64 load wrap", Atomics.load(u64, 0));
show("u64 add previous", Atomics.add(u64, 0, 2n));
show("u64 add after", Atomics.load(u64, 0));
show("u64 compare miss", Atomics.compareExchange(u64, 0, -1n, 5n));
show("u64 compare hit", Atomics.compareExchange(u64, 0, 1n, -1n));
show("u64 final", Atomics.load(u64, 0));

showThrow("bigint view number", () => Atomics.store(i64, 0, 1));
showThrow("numeric view bigint", () => Atomics.store(numeric, 0, 1n));
showThrow("compare expected number", () =>
  Atomics.compareExchange(i64, 0, 1, 1n),
);
showThrow("compare replacement number", () =>
  Atomics.compareExchange(i64, 0, 123n, 1),
);
