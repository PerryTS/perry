// A DataView (like an ArrayBuffer) has no `length` and no [Symbol.iterator]:
// Array.from takes the array-like path with length 0 and yields [], while
// spread and for-of throw "is not iterable". Typed arrays and Buffers still
// iterate their bytes.
const bytes = new Uint8Array([1, 2, 3]);
const dv = new DataView(bytes.buffer);

console.log(JSON.stringify(Array.from(dv as any)));
console.log(JSON.stringify(Array.from(dv as any, (x: unknown) => x)));
console.log(JSON.stringify(Array.from(bytes.buffer as any)));

function attempt(label: string, f: () => unknown): void {
  try {
    console.log(label, JSON.stringify(f()));
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}
attempt("spread dataview", () => [...(dv as any)]);
attempt("for-of dataview", () => {
  const out: unknown[] = [];
  for (const x of dv as any) out.push(x);
  return out;
});
attempt("spread arraybuffer", () => [...(bytes.buffer as any)]);

console.log(JSON.stringify(Array.from(bytes)));
console.log(JSON.stringify([...bytes]));
console.log(JSON.stringify(Array.from(Buffer.from([7, 8]))));
