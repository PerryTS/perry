// S0 `restrep` (I32 at rest B2): every int Array's representation is claimed
// at rest, so the rep-keyed tier decodes a double word of an Array that a
// builtin reads (never at rest). The array rep witness at the load must
// report the word.
const a = new Array<number>(16).fill(0);
for (let i = 0; i < 16; i++) a[i] = (i * 7) & 31;
let s = 0;
for (let r = 0; r < 4; r++) for (let i = 0; i < 16; i++) s = (s + (a[i] | 0)) | 0;
console.log(s, JSON.stringify(a).length);
