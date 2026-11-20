// E15: `with` resolves names on the object dynamically. perry rejects
// `with` at compile time, so E15 cannot reach a compiled program; the test
// asserts the rejection stays in place.
// WPFACTS: rejected
class Wo { constructor() { this.wf = 1; } }
var w = new Wo();
with (w) { wf = 5; }
console.log(w.wf);
