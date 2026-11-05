// HWS hook: js_runtime_install_diagnostics
// HWS prefixes: perry_runtime::process::report::|perry_runtime::gc::heap_snapshot
console.log(process.report.getReport().header.event);
