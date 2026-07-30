// #6871: `let x;` with no initializer inside a loop body was not reset to
// undefined on each iteration when the assignment happened inside a NESTED
// loop — the slot is allocated once and codegen emitted no store for
// `init: None`, so iteration N+1 still saw iteration N's value. A plain `if`
// in the same body was already correct, which is what kept this hidden.

const src: string[][] = [["a"], [], ["b"]];

// --- nested for-of ---------------------------------------------------------
const a: string[] = [];
for (let i = 0; i < src.length; i++) {
  let f: string[] | undefined;
  for (const n of src[i]) { if (!f) f = []; f.push(n); }
  a.push(f === undefined ? "u" : f.join(""));
}
console.log(a.join(" "));

// --- nested while ----------------------------------------------------------
const b: string[] = [];
for (let i = 0; i < src.length; i++) {
  let h: string[] | undefined;
  let j = 0;
  while (j < src[i].length) { if (!h) h = []; h.push(src[i][j]); j++; }
  b.push(h === undefined ? "u" : h.join(""));
}
console.log(b.join(" "));

// --- scalar accumulator ----------------------------------------------------
const c: string[] = [];
for (let i = 0; i < src.length; i++) {
  let k: number | undefined;
  for (const n of src[i]) { if (k === undefined) k = 0; k += n.length; }
  c.push(k === undefined ? "u" : String(k));
}
console.log(c.join(" "));

// --- string binding, nested for -------------------------------------------
const d: string[] = [];
for (let i = 0; i < src.length; i++) {
  let s: string | undefined;
  for (let j = 0; j < src[i].length; j++) { s = (s ?? "") + src[i][j]; }
  d.push(s === undefined ? "u" : s);
}
console.log(d.join(" "));

// --- outer while, nested for-of -------------------------------------------
const e: string[] = [];
let w = 0;
while (w < src.length) {
  let g: string[] | undefined;
  for (const n of src[w]) { if (!g) g = []; g.push(n); }
  e.push(g === undefined ? "u" : g.join(""));
  w++;
}
console.log(e.join(" "));

// --- explicit `= undefined` was always correct; keep it that way -----------
const f2: string[] = [];
for (let i = 0; i < src.length; i++) {
  let g2: string[] | undefined = undefined;
  for (const n of src[i]) { if (!g2) g2 = []; g2.push(n); }
  f2.push(g2 === undefined ? "u" : g2.join(""));
}
console.log(f2.join(" "));

// --- assignment directly in the loop body (already correct) ---------------
const g3: string[] = [];
for (let i = 0; i < 3; i++) {
  let x: string | undefined;
  if (i === 0) x = "set";
  g3.push(x === undefined ? "u" : x);
}
console.log(g3.join(" "));

// --- `var` remains one function-scoped binding after static unrolling -------
// #6876: the unroller used to refresh this declaration to a fresh LocalId for
// each copy. `var` is hoisted, though, so the value assigned in iteration zero
// must remain visible to the later iterations.
function varKeepsValue(): string {
  const values: string[] = [];
  for (let i = 0; i < 3; i++) {
    var value: string | undefined;
    if (i === 0) value = "kept";
    values.push(value === undefined ? "u" : value);
  }
  return values.join(" ");
}
console.log(varKeepsValue());

// --- declaration without init still reads undefined before assignment -----
function readsUndefined(): string {
  let z: number | undefined;
  const before = z === undefined ? "u" : String(z);
  z = 5;
  return `${before} ${z}`;
}
console.log(readsUndefined());

// --- nested loop that never runs leaves undefined every iteration ---------
const empty: number[][] = [[], [], []];
const h2: string[] = [];
for (let i = 0; i < empty.length; i++) {
  let q: number | undefined;
  for (const n of empty[i]) { q = n; }
  h2.push(q === undefined ? "u" : String(q));
}
console.log(h2.join(" "));
