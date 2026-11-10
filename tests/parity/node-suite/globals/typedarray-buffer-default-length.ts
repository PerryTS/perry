// @ts-nocheck
function show(label, value) {
  console.log(label + ":" + String(value));
}

function showErr(label, fn) {
  try {
    fn();
    console.log(label + ":NO_THROW");
  } catch (err) {
    console.log(label + ":" + err.name);
  }
}

const sab = new SharedArrayBuffer(16);
const i32Full = new Int32Array(sab);
show("sab full length", i32Full.length);
show("sab full byteLength", i32Full.byteLength);
show("sab full byteOffset", i32Full.byteOffset);
show("sab full first", i32Full[0]);
show("sab atomics wait not equal", Atomics.wait(i32Full, 0, 1, 0));
show("sab atomics notify default", Atomics.notify(i32Full, 0));

const i32Offset = new Int32Array(sab, 4);
show("sab offset length", i32Offset.length);
show("sab offset byteLength", i32Offset.byteLength);

const ab = new ArrayBuffer(10);
const u16Offset = new Uint16Array(ab, 2);
show("ab offset length", u16Offset.length);
show("ab offset byteLength", u16Offset.byteLength);

showErr("full misaligned byteLength", () => new Int32Array(new ArrayBuffer(10)));
showErr("offset misaligned", () => new Int32Array(new SharedArrayBuffer(16), 2));
showErr("offset oob", () => new Int32Array(new SharedArrayBuffer(16), 20));
