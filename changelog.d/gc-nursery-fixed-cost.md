The nursery also grows while one minor collection's fixed cost (its CPU time
minus the survivor drain) is above 2% of the thread CPU time that filled its
Eden, on its own dial next to the survivor-influx ladder (same 64 MB ceiling,
shrinks back at 1%). A program that references `v8.writeHeapSnapshot` or
`process.report` keeps the ordinary collector body until it actually takes a
snapshot or report; a build compiled with `PERRY_GC_DIAG`, `PERRY_GC_TRACE` or
`PERRY_GC_VERIFY_EVACUATION` installs the diagnostic body from the start
(`js_runtime_install_gc_diagnostics`), and `PERRY_GC_LOG` no longer changes
the collector body.
