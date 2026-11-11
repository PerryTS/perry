// VR-3a S0 arm `vrwrite` (DESIGN-p4l4 §10.3): a record written after its
// birth. Correct: R2 rejects the template, which stays a heap object, and
// the write is seen. Sabotaged (R2 ignored): the binding is its fields, the
// write goes to a materialized copy, and check mode reports the store into
// the copy with 86.
const u = { a: Math.random() > 2 ? 1 : 2, b: 3 };
(u as any).a = 5;
console.log(u.a + u.b);
