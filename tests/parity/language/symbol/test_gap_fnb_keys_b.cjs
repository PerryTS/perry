const source = require('./test_gap_fnb_keys_a.cjs');
module.exports = { same: source.alias, distinct: Symbol('same-description'), registry: Symbol.for('FNB-cross-module') };
