// Test: repsel Phase 4a.3 — module-wide barrier kill for Ptr<NumArray>.
// This module CONTAINS barriers (an indexed Array.prototype write and an
// Object.defineProperty site), so NO local may be promoted to guard-free
// access: a hole read must observe the polluted prototype, and a
// defineProperty'd index must divert reads. Everything must stay byte-exact
// vs `node --experimental-strip-types` (the guarded tiers consult the
// runtime pollution byte / descriptor bit; a wrongly-promoted local would
// return undefined/qNaN instead).

// Pollute Array.prototype AFTER some accesses, then observe through holes.
function holesSeeProto(): string {
  const c: number[] = new Array(4);
  c[0] = (c[0] || 0) + 1;
  const before = "" + c[2];
  (Array.prototype as any)[2] = 777;
  const after = "" + c[2]; // hole -> reads through the polluted prototype
  const viaOr = (c[2] as any) || -1; // 777 is truthy
  delete (Array.prototype as any)[2];
  return before + " " + after + " " + viaOr;
}
console.log(holesSeeProto());

// defineProperty accessor on an index of the SAME shape the histogram uses.
function definedIndex(): string {
  const c: number[] = new Array(4);
  c[0] = 5;
  let gets = 0;
  Object.defineProperty(c, 1, {
    get() {
      gets++;
      return 42;
    },
  });
  const sum = (c[0] || 0) + (c[1] as any) + (c[1] as any);
  return sum + " " + gets;
}
console.log(definedIndex());

// ALIASED prototype write: the pollution happens through a local holding
// `Array.prototype`, so the receiver of the indexed write is an ordinary
// local — invisible to the direct-form `.prototype[i] = …` barrier. The
// module-wide `opaque_prototype_mutation` fact (set where the prototype is
// NAMED) is what must stand the promotion down; otherwise a guard-free
// HolesOK read would return the quiet NaN where JS observes the inherited
// value.
function aliasedProtoWrite(): string {
  const c: number[] = new Array(4);
  c[0] = 1;
  const p: any = Array.prototype; // naming site -> opaque prototype mutation
  p[3] = 555;
  const viaHole = "" + c[3]; // inherited 555, NOT undefined
  const viaOr = (c[3] as any) || -1; // 555 is truthy
  const viaSum = (c[0] || 0) + ((c[3] as any) || 0);
  delete p[3];
  const afterDelete = "" + c[3];
  return viaHole + " " + viaOr + " " + viaSum + " " + afterDelete;
}
console.log(aliasedProtoWrite());

// The histogram shape still computes exactly under the module-wide kill.
function histogram(data: number[], size: number): number[] {
  const counts: number[] = new Array(size);
  const mask = size - 1;
  for (let i = 0; i < data.length; i++) {
    const v = data[i] & mask;
    counts[v] = (counts[v] || 0) + 1;
  }
  return counts;
}
const data: number[] = [];
let seed = 4242;
for (let i = 0; i < 2000; i++) {
  seed = (seed * 48271) % 2147483647;
  data.push(seed);
}
console.log(histogram(data, 16).join(","));
