'use strict';
const b = require('./b.cjs');
require.cache[require.resolve('./b.cjs')].exports.mk = (a) => -a;
module.exports = { Pt: b.Pt, mk: b.mk, note: require('./b.cjs').mk(3) };
