declare function gc(): void;
function sum(a: Float64Array): number {
  let out = 0;
  for (let i = 0; i < a.length; i++) out += a[i];
  return out;
}
const a = new Float64Array(32768);
for (let i = 0; i < a.length; i++) a[i] = i;
console.log('float', sum(a), a[32767]);
const b = new Uint32Array(32768);
for (let i = 0; i < b.length; i++) b[i] = i * 3;
const c = new Uint8Array(131072);
for (let i = 0; i < c.length; i++) c[i] = i & 255;
if (typeof gc === 'function') gc();
console.log('integer', b[32767], c[131071]);
console.log('backing', a.buffer === a.buffer, a.byteLength, b.byteLength, c.byteLength);
