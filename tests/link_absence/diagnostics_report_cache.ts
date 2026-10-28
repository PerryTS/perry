// HWS companion: diagnostics
// HWS hook: js_runtime_install_diagnostics
// HWS prefixes: perry_runtime::process::report::|perry_runtime::gc::heap_snapshot
const report=process.report;console.log(report===process.report,report.getReport().header.event);
const values=[];for(let i=0;i<20000;i++)values.push({value:i});console.log(values.length);
console.log(report===process.report,process.report.getReport().header.event);
