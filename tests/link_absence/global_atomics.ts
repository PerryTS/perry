// HWS hook: js_runtime_install_global_atomics
// HWS prefixes: perry_runtime::atomics::|js_atomics_
const a = globalThis.Atomics; console.log(a.isLockFree(4));
