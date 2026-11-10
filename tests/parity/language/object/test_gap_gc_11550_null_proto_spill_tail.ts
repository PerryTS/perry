// #11550: a `{ __proto__: null }` object's overflow ("spill") slots lost their
// values across a collection.
//
// A null-prototype object literal keeps only two inline slots (a plain `{}`
// keeps eight), so its remaining keys live in an object-owned spill buffer (a GC_TYPE_ARRAY hung off the
// object's meta record). That buffer is allocated with `length` = the requested
// capacity while its physical capacity rounds up, and the tail past `length`
// is NOT initialized: it holds whatever the arena memory last held. The first
// store into that tail read the leftover bits as the slot's "old value". When
// those bits happened to be a NaN-boxed heap pointer (very common: dead arrays
// of strings leave exactly that behind), the store looked like a
// pointer-over-pointer overwrite and skipped the GC slot-mask update. The
// collector then never visited the slot: a copying minor left the object
// holding a from-space address, a full collection freed the live string.
//
// qs builds its `parseValues` accumulator this way (`{ __proto__: null }`, one
// key per query parameter), which is how `qs.parse` returned wrong values and
// faulted under the seeded GC schedule.
//
// `poison()` leaves dead arrays full of string pointers in young memory, so the
// spill tails that follow are pointer-shaped garbage on every run; the old
// runtime then loses values on the SHIPPED DEFAULT, with no GC knobs. The plain
// `{}` rows spill too, once past their inline slots (k8 onward), and lost values
// the same way: the defect is in the spill store, not in the null prototype.

declare function gc(): void;

function mk(p: string, n: number): string {
  // A fresh heap string (decodeURIComponent defeats constant folding).
  return decodeURIComponent(p + n);
}

const KEYS = 12;

function poison(n: number): number {
  const s = mk("poison", n);
  let t = 0;
  for (let k = 0; k < 4000; k++) {
    const a = [s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s];
    t += a.length;
  }
  return t;
}

function build(nullProto: boolean, rows: number): any[] {
  const keep: any[] = [];
  for (let r = 0; r < rows; r++) {
    if (r % 50 === 0) {
      poison(r);
      if (typeof gc === "function") gc();
    }
    const obj: any = nullProto ? { __proto__: null } : {};
    for (let i = 0; i < KEYS; i++) obj[mk("k", i)] = mk("v" + r + "_", i);
    keep.push(obj);
  }
  return keep;
}

function check(label: string, keep: any[]): void {
  let bad = 0;
  let firstBad = "";
  for (let r = 0; r < keep.length; r++) {
    for (let i = 0; i < KEYS; i++) {
      const got = keep[r]["k" + i];
      if (got !== "v" + r + "_" + i) {
        if (bad === 0) firstBad = " first=[" + r + "].k" + i;
        bad++;
      }
    }
  }
  console.log(label + ": rows=" + keep.length + " bad=" + bad + firstBad);
}

const nullProto = build(true, 1000);
const plain = build(false, 1000);
if (typeof gc === "function") gc();
// Reuse whatever a lost slot pointed at, so a stale value reads as the wrong
// string rather than by luck as the right one.
let junk = 0;
for (let i = 0; i < 20000; i++) junk += mk("junk", i).length;

check("null-proto", nullProto);
check("plain", plain);
console.log("junk=" + junk);

// The keys-only view must agree too (Object.keys walks the same storage).
const keys = Object.keys(nullProto[nullProto.length - 1]);
console.log("keys=" + keys.length + " last=" + keys[keys.length - 1]);
