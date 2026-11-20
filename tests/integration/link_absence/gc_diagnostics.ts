// HWS hook: js_runtime_install_diagnostics
// HWS prefixes: perry_runtime::gc::verify::|perry_runtime::gc::telemetry::|perry_runtime::gc::diag_sites::|perry_runtime::gc::verify_diag::|perry_runtime::gc::roots::stack_maps::verify::
// HWS implementations: verify_*|gc_verify_*|classifier_verify_enabled|report_*|*_diag|panic_stale_forwarded_reference*|record_layout_*|trace_phase_*|gc_diag_enabled|gc_log_enabled|log_collection|GcCycleTrace|ChargeProbe|start_diagnostic_step|EvacuationVerifier|dump_*|trace_dump*|print_*|CopyingMinorPhaseDiag|SurvivalDiag|scanner_profile_enabled|record_scanner|note_scanner|note_stats_delta
console.log(process.report.getReport().header.event);
let a=[]; for(let i=0;i<1000;i++) a.push({i}); console.log(a.length);
