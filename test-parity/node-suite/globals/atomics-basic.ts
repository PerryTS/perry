// @ts-nocheck
function show(label, value) {
  console.log(label + ":" + String(value));
}

show("typeof Atomics", typeof Atomics);

const sab = new SharedArrayBuffer(16);
const i32 = new Int32Array(sab, 0, 4);

show("load initial", Atomics.load(i32, 0));
show("store", Atomics.store(i32, 0, 7));
show("load after store", Atomics.load(i32, 0));
show("add previous", Atomics.add(i32, 0, 5));
show("load after add", Atomics.load(i32, 0));
show("sub previous", Atomics.sub(i32, 0, 2));
show("exchange previous", Atomics.exchange(i32, 0, 3));
show("compareExchange hit", Atomics.compareExchange(i32, 0, 3, 9));
show("compareExchange miss", Atomics.compareExchange(i32, 0, 3, 11));
show("final", Atomics.load(i32, 0));
