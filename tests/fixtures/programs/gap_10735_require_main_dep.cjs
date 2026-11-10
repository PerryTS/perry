// #10735 helper: a plain required dependency. `require.main` must be the
// process ENTRY module (test_gap_10735_require_main_entry.cts), never this
// module's own `module` — that was the bug (require.main === module was
// trivially true in every compiled CommonJS module).
console.log('dep: require.main === module:', require.main === module);
exports.mainRef = require.main;
exports.tag = 'dep';
