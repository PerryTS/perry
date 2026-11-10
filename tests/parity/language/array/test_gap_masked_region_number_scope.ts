// 5L: a masked-window region fast copy admits a flow-refined local to its
// Number-local scope (the one set `local_is_number` answers from) after the
// statement that wrote a Number, and withdraws it at the first write it cannot
// prove Number (here: a numeric string, later ToInt32'd outside the region).
// Refined locals are read as Numbers inside and after the region. Output must
// match Node on the ta_i32, plain_f64 and slow (mixed-array) copies.

function unrefineToString(S: any, s: any, seed: number): number {
  let x = S[15];
  let y = S[14];
  x = (x ^ S[0] ^ seed) | 0;
  x = (((S[1] + S[2]) | 0) ^ S[3]) | 0;
  x = s;
  y = (x ^ S[4]) | 0;
  y = (((y + S[5]) | 0) ^ S[6]) | 0;
  y = (y ^ S[7]) | 0;
  return y;
}

// Refined locals read as Numbers later in the region and after it.
function refinedReads(S: any, seed: number): string {
  let x = S[15];
  let y = S[14];
  x = (x ^ S[0] ^ seed) | 0;
  y = x * 0.5 + S[1];
  x = (((S[2] + S[3]) | 0) ^ S[4]) | 0;
  y = y + x + S[5] + S[6] + S[7];
  return String(x + 0.25) + "|" + String(y);
}

// The un-refining write reads the local's Number value in its own RHS.
function unrefineReadsOld(S: any, seed: number): string {
  let x = S[15];
  x = (x ^ S[0] ^ seed) | 0;
  x = (((S[1] + S[2]) | 0) ^ S[3]) | 0;
  x = "v" + x;
  x = x + S[4] + S[5];
  x = x + S[6] + S[7];
  return x;
}

const S = new Int32Array(16);
for (let i = 0; i < 16; i++) S[i] = ((i * 2654435761) ^ (i << 28)) | 0;
const Plain: number[] = [];
for (let i = 0; i < 16; i++) Plain.push(i * 3 - 7);
const Mixed: any[] = [1, "a", 2, 3, "b", 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];

let out: string[] = [];
for (let i = 0; i < 2000; i++) {
  const a = unrefineToString(S, String(1234 + (i & 3)), i);
  const b = refinedReads(S, i);
  const c = unrefineReadsOld(S, i);
  const d = unrefineToString(Plain, "77", i);
  const e = refinedReads(Plain, i);
  if (i % 400 === 0) out.push(String(a), b, c, String(d), e);
}
out.push(String(unrefineToString(Mixed, "5", 3)), refinedReads(Mixed, 5), unrefineReadsOld(Mixed, 7));
console.log(out.join("\n"));
