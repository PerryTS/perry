Fix a whole-program facts soundness bug: a bound function made inside a
summarized function (`function bindTo(o) { return leak.bind(null, o); }`)
lost its bound `this` and bound arguments, so a target that stored them on a
global left the caller's object proven local. A bound value now owns its
parts, and every bind call that makes it copies its operands into them.
