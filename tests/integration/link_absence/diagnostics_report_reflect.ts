// HWS companion: diagnostics
// HWS hook: js_runtime_install_diagnostics
// HWS prefixes: perry_runtime::process::report::|perry_runtime::gc::heap_snapshot
const value=Reflect.get(process,"report");console.log(value.getReport().header.event);
