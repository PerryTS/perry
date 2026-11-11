// DESIGN-p4l4 R6 (VR-4b): what a record array cannot do rejects its record,
// which then stays a heap object in an ordinary Array. An index store and a
// `length` store (a record array has no holes), an Array method without an
// intrinsic, a literal with elements (no record-array birth), and one body
// over a record array and a Number array (a record array is its own
// representation).
// vr-expect: sa,sb REJECTED R6Store
// vr-expect: la,lb REJECTED R6Store
// vr-expect: ca,cb REJECTED R6
// vr-expect: ba,bb REJECTED R6ArrayHolder(birth
// vr-expect: ja,jb REJECTED R6Join
// It calls itself so that the inliner leaves the calls in place.
function len(a: unknown[], depth: number): number {
  if (depth > 0) return len(a, depth - 1);
  return a.length;
}
const s: { sa: number; sb: number }[] = [];
s.push({ sa: 1, sb: 2 });
s[2] = { sa: 3, sb: 4 };
console.log(s.length, s[0].sa, s[1], s[2].sb);
const l: { la: number; lb: number }[] = [];
l.push({ la: 1, lb: 2 });
l.push({ la: 3, lb: 4 });
l.length = 1;
console.log(l.length, l[0].lb, l[1]);
const c: { ca: number; cb: number }[] = [];
c.push({ ca: 1, cb: 2 });
c.push({ ca: 3, cb: 4 });
console.log(c.slice(1).length, c.slice(1)[0].ca);
const b: { ba: number; bb: number }[] = [{ ba: 1, bb: 2 }];
b.push({ ba: 3, bb: 4 });
console.log(b.length, b[1].bb);
const j: { ja: number; jb: number }[] = [];
j.push({ ja: 1, jb: 2 });
console.log(len(j, 1), len([1, 2, 3], 1), j[0].ja);
