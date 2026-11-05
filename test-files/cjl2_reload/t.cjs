// Re-run by `delete require.cache[T]; require(T)` (CJL2 gap test).
globalThis.__cjl2_loads = (globalThis.__cjl2_loads || 0) + 1;
exports.loads = globalThis.__cjl2_loads;
exports.parentIsEntry = module.parent === require.main;
