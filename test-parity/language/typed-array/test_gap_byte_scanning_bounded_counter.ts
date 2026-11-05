// parity-node-argv: --expose-gc
// Bounded byte indices use one guarded canonical i32 counter, preserving every miss.
export function scanBounded(bytes: Uint8Array, i: number, end: number, extra: boolean, tick: () => void): string {
  const stop = Math.min(end, end);
  let out = '';
  for (; i < stop; i++) {
    out += String(bytes[i]) + ':' + String(Number(i) + 2147483647) + ',';
    if (extra) i++;
    if (i === 2) tick();
  }
  return out + '|' + String(i) + '|' + typeof i + '|' + Object.is(i, -0);
}
const idle = () => {};
const bytes = new Uint8Array([3, 5, 7, 11]);
for (const i of [0, -0, -1, 0.5, NaN, Infinity, 2147483646]) {
  console.log('index', String(i), scanBounded(bytes, i, i === 2147483646 ? 2147483649 : 4, false, idle));
}
console.log('fractional-bound', scanBounded(bytes, 0, 3.5, false, idle));
console.log('negative-zero-exit', scanBounded(bytes, -0, -1, false, idle));
console.log('string-index', scanBounded(bytes, '0' as any, 4, false, idle));
console.log('bigint-index', scanBounded(bytes, 0n as any, 4, false, idle));
console.log('double-step', scanBounded(bytes, 0, 4, true, idle));
console.log('overflow-budget', scanBounded(bytes, 2147483646, 2147483647, true, idle));
console.log('wrong-receiver', scanBounded({0: 'a', 1: true, 2: 7} as any, 0, 3, false, idle));
const backing = new ArrayBuffer(4);
const detached = new Uint8Array(backing);
detached.set([3, 5, 7, 11]);
console.log('detach', scanBounded(detached, 0, 4, false, () => { backing.transfer(); }));
const resizable = new ArrayBuffer(4, {maxByteLength: 8});
const shrinking = new Uint8Array(resizable);
shrinking.set([3, 5, 7, 11]);
console.log('resize', scanBounded(shrinking, 0, 4, false, () => { resizable.resize(3); }));
const gc = (globalThis as any).gc;
function collect() { if (typeof gc === 'function') gc(); }
function freshOffset(): Uint8Array {
  const owner = new ArrayBuffer(128);
  return new Uint8Array(owner, 32, 8);
}
const offset = freshOffset();
offset.set([1, 2, 3, 4, 5, 6, 7, 8]);
console.log('owner-gc', scanBounded(offset, 0, 8, false, collect));

export function scanOffset(bytes: Uint8Array, i: number, end: number): number {
  const stop = Math.min(end, end);
  for (; i < stop; i++) { bytes[i + 1]; i | 0; }
  return i;
}
console.log('computed-index', scanOffset(bytes, 0, 3));

export function scanStep(bytes: Uint8Array, i: number, end: number): number {
  const stop = Math.min(end, end);
  for (; i < stop; i++) { bytes[++i]; i | 0; }
  return i;
}
console.log('prefix-index', scanStep(bytes, 0, 3));
