'use strict';
const b0 = require('./b.cjs');
const n = process.argv.length > 99 ? 'module' : './b.cjs';
const b = require(n);
module.exports = { Pt: b.Pt, mk: b.mk, note: [b === b0, typeof require(process.argv.length > 99 ? process.argv[2] : 'path').join].join(',') };
