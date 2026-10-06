// Each retained one-byte view keeps one distinct 8 KiB pool alive.
declare function gc(): void;
Buffer.poolSize = 8192;
// Drain Node's preexisting default pool before touching a pool's whole store.
for (let i = 0; i < 32; i++) Buffer.allocUnsafe(4095);
gc();
const count = Number(process.argv[2] || '128');
const retained: Buffer[] = [];
for (let i = 0; i < count; i++) {
  const small = Buffer.allocUnsafe(1);
  const store = small.buffer;
  if (store.byteLength !== 8192) throw new Error('unexpected pool size');
  new Uint8Array(store).fill(i & 255);
  retained.push(small);
  // Exhaust this pool; the next retained view belongs to a fresh owner.
  Buffer.allocUnsafe(4095);
  Buffer.allocUnsafe(4095);
}
gc();
let checksum = 0;
let stores = 0;
for (let i = 0; i < retained.length; i++) {
  checksum += retained[i][0];
  if (i === 0 || retained[i].buffer !== retained[i - 1].buffer) stores++;
}
console.log('views=' + count + ' stores=' + stores + ' checksum=' + checksum);
