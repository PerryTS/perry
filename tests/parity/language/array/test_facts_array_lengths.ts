// Array literal lengths the whole-program facts track (wpfacts
// `solve::arr_len`): a fixed table's for-of and `.length` reads, beside
// every way a program can move a length after birth.
const KNIGHT = [-33, -31, -18, -14, 14, 18, 31, 33];
const PUSHED = [1, 2, 3];
const STORED = [1, 2, 3];
const ASSIGNED = [1, 2];
const DEFINED = [1, 2, 3, 4];
const SPLICED = [1, 2, 3];
const SHRUNK = [1, 2, 3, 4, 5];
const KEYED = [1, 2];

function knightSum(square: number): number {
  let s = 0;
  for (const d of KNIGHT) s += (square + d) & 0x88 ? 0 : d;
  const k = KNIGHT;
  return s * 1000 + k.length;
}

function lengths(): string {
  const out: number[] = [];
  for (const a of [PUSHED, STORED, ASSIGNED, DEFINED, SPLICED, SHRUNK, KEYED]) {
    let n = 0;
    for (const x of a) n += x === undefined ? 100 : 1;
    const local = a;
    out.push(local.length, n);
  }
  return out.join(",");
}

function grow(i: number): void {
  STORED[i] = 9;
}

console.log(knightSum(0x22), knightSum(0x00), knightSum(0x77));
PUSHED.push(4);
grow(1);
grow(6);
Object.assign(ASSIGNED, [5, 6, 7, 8]);
Object.defineProperty(DEFINED, "length", { value: 2 });
const alias: number[] = SPLICED;
alias.splice(1, 0, 7, 7);
SHRUNK.length = 3;
const key = String(KEYED.length + 3);
(KEYED as any)[key] = 1;
console.log(lengths());
console.log(knightSum(0x44));
