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
const i32 = new Int32Array(sab, 0, 2);

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
showErr("waitAsync oob", () => Atomics.waitAsync(i32, 99, 0, 0));
