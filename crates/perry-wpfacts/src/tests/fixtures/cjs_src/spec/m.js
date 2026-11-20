const { Pt, mk } = require('./a.js');
let s = 0;
for (let i = 0; i < process.argv.length + 3; i++) { const p = new Pt(i, 1); s += p.sum() + p.x + mk(i); }
console.log(s);
