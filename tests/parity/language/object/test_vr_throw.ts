// DESIGN-p4l4 B2: a thrown record would be boxed for the unwinder (R5Throw).
// vr-expect: ta,tb REJECTED R5Throw
function mk(a: number, b: number) {
  return { ta: a, tb: b };
}
try {
  throw mk(1, 2);
} catch (e) {
  console.log((e as { ta: number }).ta, e);
}
