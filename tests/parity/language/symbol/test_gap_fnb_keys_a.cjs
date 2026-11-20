const unique = Symbol('same-description');
const registry = Symbol.for('FNB-cross-module');
module.exports = { unique, alias: unique, registry };
