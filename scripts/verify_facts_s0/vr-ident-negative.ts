// VR-2 S0 arm `vrident` (DESIGN-p4l4 §10.3): two records from one site are
// compared by identity. Correct: the template is rejected (R4Identity) and
// nothing is claimed. Sabotaged (`===` ignored): it is admitted, its
// instances are claimed at birth, and the compiled `===` aborts with 86.
function mk(n: number) {
  return { a: n, b: n > 1 };
}
const u1 = mk(1);
const u2 = mk(2);
console.log(u1 === u2 ? "same" : "different", u1.a + u2.a);
