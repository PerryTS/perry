// #10735 helper: required two levels deep (entry -> dep2 -> deep). Still not
// the entry, so `require.main` must still resolve to the ENTRY's module, not
// to dep2's or to this module's own.
console.log('deep (2 levels): require.main === module:', require.main === module);
exports.tag = 'deep';
