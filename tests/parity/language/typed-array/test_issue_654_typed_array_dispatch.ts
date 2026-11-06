const a = new Float64Array(5);
for (let i = 0; i < 5; i++) a[i] = 5 - i;

console.log('a:', a);
console.log('typeof a:', typeof a);

a.sort();
console.log('sorted:', a);

console.log('a.at(0):', a.at(0));
console.log('a.at(-1):', a.at(-1));
console.log('a.length:', a.length);

const b = new Float64Array(a);
console.log('b:', b, 'len:', b.length);

const c = new Int32Array([10, 20, 30]);
const d = new Float64Array(c);
console.log('d (Float64 from Int32):', d, 'len:', d.length);

const e = new Float64Array([3.5, 1.5, 2.5]);
const f = e.toSorted();
console.log('e (orig):', e);
console.log('f (toSorted):', f);

const g = new Int8Array(4);
for (let i = 0; i < 4; i++) g[i] = i + 1;
g.sort((x, y) => y - x);
console.log('g (Int8 desc):', g);
