// #7640: computed read/write arms that lowered a receiver, then lowered more
// user code, then used the receiver. Behaviour must be unchanged by the rooting
// repair — this pins the semantics on every arm the change touches, including
// the section-D statement reorders (the key's SSO unbox now runs before the
// receiver's raw-pointer derivation).

function alloc(n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) s += String(i % 10);
  return s;
}

// --- string receiver, side-effecting index (`s[f()]`) -----------------------
const s = "abcdef";
let calls = 0;
function idx(): number {
  calls++;
  alloc(200);
  return 2;
}
console.log(s[idx()], calls);

// --- array receiver, non-numeric computed key ------------------------------
const arr: number[] = [10, 20, 30];
const anyArr: any = arr;
anyArr.note = "hi";
function keyOf(): string {
  alloc(200);
  return "note";
}
console.log(arr[keyOf() as any]);

// --- array receiver, numeric-typed but unproven index ----------------------
function dynIndex(): number {
  alloc(200);
  return 1;
}
console.log(arr[dynIndex()]);

// --- symbol key on an array and on a typed array ---------------------------
console.log(typeof arr[Symbol.iterator]);

// --- SSO string key on an object, read and write ---------------------------
// The key is short enough to be stored inline (SSO), so `unbox_str_handle`
// materialises it into a fresh heap StringHeader — the allocation section D
// reorders around.
const obj: Record<string, number> = {};
const shortKey = "ab";
obj[shortKey] = 7;
console.log(obj[shortKey]);
const dyn: any = "ab";
obj[dyn] = 9;
console.log(obj[dyn], JSON.stringify(obj));

// --- globalThis[key] = v ----------------------------------------------------
const g: any = globalThis;
const gk: any = "perryGapKey";
g[gk] = { v: 1 };
console.log(JSON.stringify(g[gk]));

// --- array with a string key ------------------------------------------------
const a2: any[] = [1, 2];
const sk: any = "extra";
a2[sk] = { z: 3 };
console.log(JSON.stringify(a2[sk]), a2.length);

// --- the byte-read fast path stays a byte read ------------------------------
const u8 = new Uint8Array([4, 5, 6, 7]);
let sum = 0;
for (let i = 0; i < u8.length; i++) sum += u8[i];
console.log(sum);
