// @ts-nocheck
function show(label, value) {
  console.log(label + ":" + String(value));
}

function showErr(label, fn) {
  try {
    fn();
    show(label, "ok");
  } catch (err) {
    show(label, err.name);
  }
}

const sab = new SharedArrayBuffer(8);
const ab = new ArrayBuffer(8);
const i32 = new Int32Array(sab, 0, 2);
const localI32 = new Int32Array(ab, 0, 2);

show("typeof waitAsync", typeof Atomics.waitAsync);
show("waitAsync length", Atomics.waitAsync.length);

const mismatch = Atomics.waitAsync(i32, 0, 1, 0);
show("mismatch keys", Object.keys(mismatch).join("|"));
show("mismatch async", mismatch.async);
show("mismatch value", mismatch.value);

const zero = Atomics.waitAsync(i32, 0, 0, 0);
show("zero async", zero.async);
show("zero value", zero.value);

const negative = Atomics.waitAsync(i32, 0, 0, -1);
show("negative async", negative.async);
show("negative value", negative.value);

showErr("waitAsync uint8 shared", () => Atomics.waitAsync(new Uint8Array(sab, 0, 8), 0, 0, 0));
showErr("waitAsync i32 nonshared", () => Atomics.waitAsync(localI32, 0, 0, 0));
showErr("waitAsync oob", () => Atomics.waitAsync(i32, 99, 0, 0));

const bigSab = new SharedArrayBuffer(16);
const bigAb = new ArrayBuffer(16);
const bigI64 = new BigInt64Array(bigSab, 0, 2);
const localBigI64 = new BigInt64Array(bigAb, 0, 2);
const bigU64 = new BigUint64Array(bigSab, 0, 2);

const bigMismatch = Atomics.waitAsync(bigI64, 0, 1n, 0);
show("big mismatch keys", Object.keys(bigMismatch).join("|"));
show("big mismatch async", bigMismatch.async);
show("big mismatch value", bigMismatch.value);

const bigZero = Atomics.waitAsync(bigI64, 0, 0n, 0);
show("big zero async", bigZero.async);
show("big zero value", bigZero.value);

const bigNegative = Atomics.waitAsync(bigI64, 0, 0n, -1);
show("big negative async", bigNegative.async);
show("big negative value", bigNegative.value);

showErr("waitAsync big number expected", () => Atomics.waitAsync(bigI64, 0, 0, 0));
showErr("waitAsync big nonshared", () => Atomics.waitAsync(localBigI64, 0, 0n, 0));
showErr("waitAsync biguint shared", () => Atomics.waitAsync(bigU64, 0, 0n, 0));
