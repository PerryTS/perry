// Uint8Array and Buffer storage detached by a callee on the loop back edge,
// structuredClone with a transfer list, a subarray over a detached parent,
// and views over a resizable ArrayBuffer that shrinks mid-loop.
function size(): number { return 12; }
function detU8(t: Uint8Array): void { (t.buffer as any).transfer(); }

function u8literal(k: number): number {
  const t = new Uint8Array(12);
  for (let i = 0; i < 12; i++) t[i] = i + 1;
  let s = 0;
  for (let i = 0; i < 12; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) detU8(t);
  }
  return s + t.length;
}
function u8computed(k: number): number {
  const n = size();
  const t = new Uint8Array(n * 1);
  for (let i = 0; i < n; i++) t[i] = i + 1;
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) detU8(t);
  }
  return s + t.length;
}
function bufalloc(k: number): number {
  const t = Buffer.alloc(12);
  for (let i = 0; i < 12; i++) t[i] = i + 1;
  let s = 0;
  for (let i = 0; i < 12; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) detU8(t);
  }
  return s + t.length;
}
function bufparam(t: Buffer, k: number): number {
  let s = 0;
  for (let i = 0; i < t.length; i++) {
    const v = t[i];
    s += v === undefined ? 1000 : v;
    if (i === k) detU8(t);
  }
  return s + t.length;
}
function coherent(): string {
  const t = new Uint8Array(8);
  const w = new Uint8Array(t.buffer);
  w[1] = 9;
  t[2] = 4;
  return t[1] + " " + w[2];
}
let acc = 0;
for (let j = 0; j < 300; j++) acc += u8literal(j % 13) + u8computed(j % 13) + bufalloc(j % 13);
const bp = Buffer.alloc(12);
for (let i = 0; i < 12; i++) bp[i] = i;
console.log(u8literal(4), u8computed(4), bufalloc(4), bufparam(bp, 3), coherent(), acc);

function structured(): string {
  const n = size();
  const a = new Int16Array(n * 3);
  for (let i = 0; i < a.length; i++) a[i] = i * 3;
  let s1 = 0;
  for (let i = 0; i < n * 3; i++) s1 += a[i];
  const cloned = structuredClone(a.buffer, { transfer: [a.buffer] });
  let s2 = 0, u = 0;
  for (let i = 0; i < n * 3; i++) { const v = a[i]; if (v === undefined) u++; else s2 += v; }
  return s1 + " " + s2 + " " + u + " " + a.length + " " + new Int16Array(cloned)[4];
}
console.log("structuredClone:", structured());

function sub(): string {
  const p = new Uint16Array(size() * 1);
  for (let i = 0; i < p.length; i++) p[i] = i + 1;
  const s = p.subarray(2, 6);
  let acc2 = 0;
  for (let i = 0; i < 4; i++) acc2 += s[i];
  (p.buffer as any).transfer();
  let after = 0;
  for (let i = 0; i < 4; i++) { const v = s[i]; after += v === undefined ? 1 : 100; }
  return acc2 + " " + after + " " + s.length + " " + p.length;
}
console.log("subarray:", sub());

function resizable(): string {
  const n = size();
  const rab = new (ArrayBuffer as any)(n * 8, { maxByteLength: 128 });
  const t = new Float64Array(rab);
  const f = new Float64Array(rab, 0, 6);
  for (let i = 0; i < t.length; i++) t[i] = i + 0.5;
  let s = 0;
  for (let i = 0; i < n; i++) {
    if (i === 4) rab.resize(16);
    const v = t[i];
    s += v === undefined ? 100 : v;
  }
  const mid = s + " " + t.length + " " + f.length + " " + f[0] + " " + t[1] + " " + t[2];
  rab.resize(64);
  return mid + " | " + t.length + " " + f.length + " " + t[7] + " " + f[5];
}
console.log("resizable:", resizable());
