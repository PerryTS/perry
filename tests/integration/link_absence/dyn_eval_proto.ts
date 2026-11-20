// HWS companion: dyn_eval
// HWS hook: js_runtime_install_dyn_eval
// HWS prefixes: perry_runtime::dyn_eval::|perry_runtime::dyn_eval_hooks::
const Run=Function;const run=Run("globalThis.hwsMarker=42");run();
console.log((globalThis as any).hwsMarker);delete (globalThis as any).hwsMarker;
try { Run("throw new Error('interpreted')")(); } catch (error) { console.log(error.message); }
