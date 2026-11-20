class Holder {
  lane: Int32Array;
  constructor(backing: ArrayBuffer) { this.lane = new Int32Array(backing); }
}
function put(h: Holder, i: number, value: number) { h.lane[i] = value; }
function read(h: Holder, i: number) { return h.lane[i]; }
const backing = new ArrayBuffer(64);
const holder = new Holder(backing);
const alias = new Uint8Array(backing);
let sum = 0;
for (let i = 0; i < 2000; i++) {
  const text = 'temporary-' + i;
  const waste = [i, i + 1, i + 2];
  put(holder, i & 15, i - 1000);
  sum += read(holder, i & 15);
  if ((i & 63) === 0 && typeof (globalThis as any).gc === 'function') (globalThis as any).gc();
  if (waste[2] !== i + 2 || text.length < 1) console.log('bad-object');
}
console.log(sum, holder.lane[15], alias.length);
const transfer = new ArrayBuffer(8);
const view = new Int32Array(transfer);
let effects = 0;
view[0] = {valueOf() { effects++; transfer.transfer(); return 123; }} as any;
console.log('detach-coercion', effects, view[0]);
