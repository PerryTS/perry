// @ts-nocheck
function show(label, value) {
  console.log(label + ":" + String(value));
}

show("typeof and", typeof Atomics.and);
show("typeof or", typeof Atomics.or);
show("typeof xor", typeof Atomics.xor);
show("lengths", Atomics.and.length + "|" + Atomics.or.length + "|" + Atomics.xor.length);

const sab = new SharedArrayBuffer(8);
const i32 = new Int32Array(sab, 0, 2);

i32[0] = 0b1100;
show("and previous", Atomics.and(i32, 0, 0b1010));
show("and after", Atomics.load(i32, 0));

show("or previous", Atomics.or(i32, 0, 0b0101));
show("or after", Atomics.load(i32, 0));

show("xor previous", Atomics.xor(i32, 0, 0b1111));
show("xor after", Atomics.load(i32, 0));

i32[1] = -1;
show("int32 high xor previous", Atomics.xor(i32, 1, 0x80000000));
show("int32 high xor after", Atomics.load(i32, 1));

const u8 = new Uint8Array(sab);
u8[4] = 0xf0;
show("u8 and previous", Atomics.and(u8, 4, 0x3f));
show("u8 and after", Atomics.load(u8, 4));
