// Counts its own evaluations without any loader capability (a named
// `process` property), so the requiring program's only one is its delete.
process.env.CJL_RELOAD_LOADS = String(Number(process.env.CJL_RELOAD_LOADS || "0") + 1);
exports.loads = Number(process.env.CJL_RELOAD_LOADS);
