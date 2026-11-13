// `Buffer.prototype.subarray` / `slice` return a Buffer over the receiver's
// bytes (%TypedArray%.prototype.subarray makes one of the receiver's kind),
// `fill` / `swap16` return the receiver. The facts once modelled the view as
// a container of primitives: a read through it held nothing, and
// `sum += v.readInt32BE(..)` was proved to stay 0. An operand that holds
// nothing (`...args` still reaches the undescribed path) gives no interval.
function scan(v: Buffer): number {
  let sum = 0;
  for (let i = 0; i < 4; i++) sum += v.readInt32BE(i * 4);
  return sum;
}
const b = Buffer.alloc(32).subarray(16);
b.writeInt32BE(5, 0);
console.log(scan(b));

const base = Buffer.alloc(32);
const s = base.slice(8, 24);
s.writeInt32BE(7, 4);
console.log(scan(s), base.readInt32BE(12));

const f = Buffer.alloc(16);
console.log(scan(f.fill(1)), f.fill(2) === f);

const w = Buffer.alloc(16);
w.writeInt16BE(0x0102, 0);
console.log(scan(w.swap16()), w.swap16() === w);

const args: [number, number] = [4, 20];
const viaSpread = Buffer.alloc(32).subarray(...args);
viaSpread.writeInt32BE(9, 12);
console.log(scan(viaSpread));
