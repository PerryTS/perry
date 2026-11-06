// #4365 — DataView.prototype.getBigInt64 / setBigInt64 / getBigUint64 /
// setBigUint64. Reads return a `bigint`; writes ToBigInt-coerce the value (a
// Number throws TypeError), store the raw 8 bytes with the requested
// endianness (big-endian default), and the methods are reflectable on the
// prototype and invokable via `.call`.

const dv = new DataView(new ArrayBuffer(64));

// Signed round-trip incl. negative + full-width unsigned (would truncate f64).
dv.setBigInt64(0, -5n);
console.log("i64:", dv.getBigInt64(0), typeof dv.getBigInt64(0));
dv.setBigUint64(8, 18446744073709551615n);
console.log("u64max:", dv.getBigUint64(8));

// Endianness: default big-endian, explicit little-endian flag.
dv.setBigInt64(16, 1n, true);
console.log("le/be:", dv.getBigInt64(16, true), dv.getBigInt64(16, false));
dv.setBigInt64(24, -1n, false);
console.log("be -1:", dv.getBigInt64(24, false), dv.getBigUint64(24, false));

// Boolean coerces via ToBigInt (true -> 1n).
dv.setBigUint64(32, true as unknown as bigint);
console.log("bool:", dv.getBigUint64(32));

// A Number is not convertible: ToBigInt throws a TypeError, no write.
try {
  dv.setBigInt64(40, 7 as unknown as bigint);
  console.log("number: no throw");
} catch (e) {
  const err = e as Error;
  console.log("number:", err.name, "-", err.message);
}

// Out-of-bounds byte offset → RangeError (8-byte access past the buffer end).
try {
  dv.getBigInt64(60);
  console.log("oob-get: no throw");
} catch (e) {
  console.log("oob-get:", (e as Error).name);
}
try {
  dv.setBigUint64(60, 1n);
  console.log("oob-set: no throw");
} catch (e) {
  console.log("oob-set:", (e as Error).name);
}

// Reflectable on the prototype, callable via `.call`.
console.log(
  "proto:",
  typeof DataView.prototype.getBigInt64,
  typeof DataView.prototype.setBigUint64,
);
const dv2 = new DataView(new ArrayBuffer(8));
DataView.prototype.setBigInt64.call(dv2, 0, 42n);
console.log("call:", DataView.prototype.getBigInt64.call(dv2, 0));

// Brand check: invoking on a non-DataView receiver throws a TypeError.
try {
  DataView.prototype.getBigInt64.call({} as DataView, 0);
  console.log("brand: no throw");
} catch (e) {
  console.log("brand:", (e as Error).name);
}
