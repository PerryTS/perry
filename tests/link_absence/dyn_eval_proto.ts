// HWS companion: dyn_eval
// HWS hook: js_runtime_install_dyn_eval
// HWS prefixes: perry_runtime::dyn_eval::|perry_runtime::dyn_eval_hooks::
const Run=Function;const run=Run("Object.prototype.hwsMarker=42");run();
const object:any={};console.log(object.hwsMarker);delete (Object.prototype as any).hwsMarker;
try { Run("throw new Error('interpreted')")(); } catch (error) { console.log(error.message); }
