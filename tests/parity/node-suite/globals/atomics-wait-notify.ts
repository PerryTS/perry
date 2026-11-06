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

show("typeof isLockFree", typeof Atomics.isLockFree);
show("typeof wait", typeof Atomics.wait);
show("typeof notify", typeof Atomics.notify);
show("isLockFree length", Atomics.isLockFree.length);
show("wait length", Atomics.wait.length);
show("notify length", Atomics.notify.length);

for (const value of [undefined, NaN, -1, 0, 1, 2, 3, 4, 8, 16, "4", 4.9, Infinity]) {
  show("isLockFree " + String(value), Atomics.isLockFree(value));
}

const sab = new SharedArrayBuffer(16);
const i32 = new Int32Array(sab);

show("wait not equal", Atomics.wait(i32, 0, 1, 0));
show("wait zero timeout", Atomics.wait(i32, 0, 0, 0));
show("wait negative timeout", Atomics.wait(i32, 0, 0, -1));
show("notify default", Atomics.notify(i32, 0));
show("notify zero", Atomics.notify(i32, 0, 0));
show("notify string count", Atomics.notify(i32, 0, "2"));

showErr("wait uint8 shared", () => Atomics.wait(new Uint8Array(sab), 0, 0, 0));
showErr("notify uint8 shared", () => Atomics.notify(new Uint8Array(sab), 0));
showErr("wait oob", () => Atomics.wait(i32, 99, 0, 0));
showErr("notify oob", () => Atomics.notify(i32, 99));
