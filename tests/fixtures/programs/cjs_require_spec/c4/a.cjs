'use strict';
const b = require('./b.cjs');
for (const c of module.children) c.exports.mk = (a) => a + 100;
module.exports = { Pt: b.Pt, mk: b.mk, note: module.children.length };
