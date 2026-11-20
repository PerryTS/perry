**GC: actually register the process-emitter root scanner.**

#8294 added `process_emitter_root_scanner` and a
`register_process_emitter_root_scanner()` wrapper, but nothing ever called the
wrapper — so the scanner was never registered and process `EventEmitter`
listener closures, held as raw `*const ClosureHeader` in a TLS map, remained
invisible to the collector. The fix was inert. `gc_init` now calls it, and the
census reports 96 holders reached by a registered scanner where it previously
reported 94.

`rustc` had been saying so: `function 'register_process_emitter_root_scanner'
is never used` was one of five dead-code errors that made `main` red under
`RUSTFLAGS="-D warnings"`. The others were unused frame-walk locals in
`native_stack_scan.rs`. All cleared.
