// HWS companion: diagnostics
// HWS hook: js_runtime_install_diagnostics
// HWS prefixes: perry_runtime::process::report::|perry_runtime::gc::heap_snapshot
const processValue:any=process;const name=["re","port"].join("");
console.log(processValue[name].getReport().header.event);
