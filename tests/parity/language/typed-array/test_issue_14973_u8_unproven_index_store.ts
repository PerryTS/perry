// #14973: `HEAP[p++] = c` into a module-level Uint8Array whose index has no
// static integer proof (it starts from a `number` parameter) stores the byte
// inline once the index tests as an in-bounds integer at run time. Every other
// key keeps the complete [[Set]]: fractional, -0, negative, NaN, Infinity and
// out-of-range keys, on owning arrays and on offset views of an ArrayBuffer.
const HEAP = new Uint8Array(new ArrayBuffer(16));
const OWNED = new Uint8Array(8);
const ab = new ArrayBuffer(12);
const VIEW = new Uint8Array(ab, 4, 4);

function encode(s: string, ptr: number): number {
  let p = ptr;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0x80) HEAP[p++] = c;
    else { HEAP[p++] = 0xc0 | (c >> 6); HEAP[p++] = 0x80 | (c & 63); }
  }
  HEAP[p++] = 0;
  return p - ptr;
}

function putOwned(k: number, v: number): number { return (OWNED[k] = v); }
function putView(k: number, v: number): number { return (VIEW[k] = v); }

console.log("encode", encode("Aé!", 0), encode("hi", 9), Array.from(HEAP).join(","));
console.log("encode tail", encode("xyz", 14), Array.from(HEAP).join(","));

const keys = [0, 1.5, -0, -1, NaN, Infinity, -Infinity, 7, 8, 1e10, 2147483648, 3];
const results: string[] = [];
for (let n = 0; n < keys.length; n++) results.push(String(putOwned(keys[n], 200 + n)));
console.log("results", results.join(","));
console.log("owned", Array.from(OWNED).join(","), JSON.stringify(Object.keys(OWNED)));
console.log("own keys", (OWNED as any)["1.5"], (OWNED as any)["-1"], (OWNED as any)[8]);

const wide = [256, 511, -1, 1.9, -1.9, 2147483647, 4294967297.5, NaN, Infinity, -0];
const stored: string[] = [];
for (let n = 0; n < wide.length; n++) { putOwned(5, wide[n]); stored.push(String(OWNED[5])); }
console.log("values", stored.join(","));

for (let k = -1; k <= 4; k++) putView(k, 10 + k);
putView(0.5, 99);
console.log("view", Array.from(VIEW).join(","), Array.from(new Uint8Array(ab)).join(","));
