// #10735 helper for the ESM-entry case: a CJS module reached only via
// `import` from an ESM entry has NO CommonJS "main" — Node leaves
// `require.main` as `undefined` there (verified against Node 26.5.1; this is
// not assumed).
console.log('esm-imported dep: require.main === undefined:', require.main === undefined);
console.log('esm-imported dep: typeof require.main:', typeof require.main);
module.exports = { tag: 'esm-dep' };
