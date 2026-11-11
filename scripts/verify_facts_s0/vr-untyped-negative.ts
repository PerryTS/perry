// VR-3b S0 arm `vruntyped` (DESIGN-p4l4 §10.3): a record passed by a call
// that is no typed call of one body. Correct: R5UntypedCall rejects the
// template, which stays a heap object. Sabotaged (the rule ignored): `use`
// takes the record as its lanes, the untyped call hands a copy to `use`'s
// boxed entry, and check mode reports the record there with 86.
// The helpers call themselves so that the inliner leaves the calls in place.
function use(r: { a: number; b: number }, depth: number): number {
  if (depth > 0) return use(r, depth - 1);
  return r.a + r.b;
}
function other(r: { a: number; b: number }, depth: number): number {
  if (depth > 0) return other(r, depth - 1);
  return r.a - r.b;
}
const u = { a: Math.random() > 2 ? 1 : 2, b: 3 };
const fs = [use, other];
console.log(fs[u.b - 3](u, 1) + use(u, 1));
