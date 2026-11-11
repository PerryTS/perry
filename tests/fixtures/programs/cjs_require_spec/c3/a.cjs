'use strict';
const b = require('./b.cjs');
const keys = new Function('r', 'return Object.keys(r("module")._cache).length')(require);
module.exports = { Pt: b.Pt, mk: b.mk, note: keys > 0 };
