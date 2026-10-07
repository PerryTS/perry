declare function gc(): void;
function scan(view: Uint32Array): number {
  let sum = 0;
  for (let i = 0; i < view.length; i++) {
    if ((i & 7) === 0) gc();
    sum += view[i];
  }
  return sum;
}
function makeView(): Uint32Array {
  const owner = new ArrayBuffer(256 + 16);
  const view = new Uint32Array(owner, 16, 64);
  for (let i = 0; i < view.length; i++) view[i] = i + 1;
  return view;
}
console.log(scan(makeView()));
function scanLocal(): number {
  const view = new Uint32Array(64);
  for (let i = 0; i < 64; i++) view[i] = i + 1;
  let sum = 0;
  for (let i = 0; i < 64; i++) {
    if ((i & 7) === 0) gc();
    sum += view[i];
  }
  return sum;
}
console.log(scanLocal());
function scanBuffer(view: Buffer): number {
  let sum = 0;
  for (let i = 0; i < 16; i++) {
    if ((i & 3) === 0) gc();
    sum += view.readInt32BE(i * 4);
  }
  return sum;
}
function makeBufferView(): Buffer {
  const owner = Buffer.alloc(80);
  const view = owner.subarray(16);
  for (let i = 0; i < 16; i++) view.writeInt32BE(i + 1, i * 4);
  return view;
}
console.log(scanBuffer(makeBufferView()));
const bagged = makeBufferView();
(bagged as any).label = 'keeps owner';
console.log(scanBuffer(bagged));
