// A require call resolves per string constant of its specifier (R-SPEC):
// fixture tests/fixtures/programs/cjs_require_spec/c4 (a.cjs requires b.cjs).
const { Pt, mk, note } = require('../../../fixtures/programs/cjs_require_spec/c4/a.cjs');
let s = 0;
for (let i = 0; i < process.argv.length + 3; i++) { const p = new Pt(i, 1); s += p.sum() + p.x + mk(i); }
console.log(s, note);
