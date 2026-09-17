// The case the elision has to survive: a slot that HELD A POINTER, was popped,
// and is then re-filled with a plain number. Skipping the layout note leaves
// the slot's pointer-mask bit set over a numeric word, so the collector must
// re-validate slot words rather than trust the mask.
class Node2 { v: number; tag: string; constructor(v: number) { this.v = v; this.tag = "n" + v; } }
const retained: any[] = [];
let checksum = 0;
for (let round = 0; round < 400; round++) {
  const a: any[] = [];
  // fill with pointers, then drain
  for (let i = 0; i < 24; i++) a.push(new Node2(round * 100 + i));
  while (a.length > 0) { const n = a.pop(); checksum += n.v % 7; }
  // refill the SAME slots with plain numbers
  for (let i = 0; i < 24; i++) a.push(i * 1.5);
  while (a.length > 4) a.pop();
  // interleave: numbers and pointers into one array
  for (let i = 0; i < 12; i++) { a.push(i); a.push(new Node2(i)); }
  for (const e of a) { if (typeof e === "number") checksum += e; else checksum += e.v % 5; }
  if (round % 40 === 0) retained.push(a);
}
console.log("checksum", checksum, "retained", retained.length);
let live = 0;
for (const a of retained) for (const e of a) if (typeof e !== "number" && e && e.tag) live++;
console.log("live tags", live);
