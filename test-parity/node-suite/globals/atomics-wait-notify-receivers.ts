// @ts-nocheck
function show(label, value) {
  console.log(label + ":" + String(value));
}

function showErr(label, fn) {
  try {
    show(label, fn());
  } catch (err) {
    console.log(label + ":" + err.name);
  }
}

const sab = new SharedArrayBuffer(8);
const ab = new ArrayBuffer(8);

const sharedI32 = new Int32Array(sab);
const localI32 = new Int32Array(ab);
const sharedBigI64 = new BigInt64Array(sab);
const localBigI64 = new BigInt64Array(ab);
const sharedBigU64 = new BigUint64Array(sab);

show("wait i32 shared mismatch", Atomics.wait(sharedI32, 0, 1, 0));
showErr("wait i32 nonshared", () => Atomics.wait(localI32, 0, 0, 0));
show("notify i32 nonshared", Atomics.notify(localI32, 0));
showErr("notify i32 nonshared oob", () => Atomics.notify(localI32, 9));

show("wait b64 shared mismatch", Atomics.wait(sharedBigI64, 0, 1n, 0));
show("wait b64 shared timeout", Atomics.wait(sharedBigI64, 0, 0n, 0));
showErr("wait b64 number expected", () => Atomics.wait(sharedBigI64, 0, 0, 0));
showErr("wait b64 nonshared", () => Atomics.wait(localBigI64, 0, 0n, 0));
show("notify b64 shared", Atomics.notify(sharedBigI64, 0));
show("notify b64 nonshared", Atomics.notify(localBigI64, 0));
showErr("notify b64 nonshared oob", () => Atomics.notify(localBigI64, 9));

showErr("wait bu64 shared", () => Atomics.wait(sharedBigU64, 0, 0n, 0));
showErr("notify bu64 shared", () => Atomics.notify(sharedBigU64, 0));
