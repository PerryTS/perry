# Source-length R4 validation

Source 341c99f7907326c2da7500a2091f39a3243fddcc, package 0.5.1530.
All 286 release JSON tests pass, single-threaded, with frozen source hashes.
The matched compiler/runtime-static/stdlib-static build passes (5m43s).
Raw-handle, address, runtime-root, formatting and file-size gates pass.

Compiled output matches Node under scheduled/protected/full GC and retained
output lifetime checks. The scan witnesses 1323 protected copying minors;
1000 changing Unicode parses witness 26 ordinary copying minors. Malloc-sweep
cadence remains 39,40,40,40 versus the corrected R2 reference. All numeric RESULT
fields in this driver are finite. The last escaped-key fixture has a numeric
id for scan/sparse, preserving its duplicate keys; all fourteen compiled cases
match Node, while twelve fail against the immutable unfixed-main control.

provenance.json pins the source, immutable object files, matched compiler and
archives, and newly linked workers. source.patch is relative to the corrected
R2 source. validate-recorded.txt preserves the exact local validation driver;
it records a build procedure with local prerequisites, not a portable entry
point. Performance requires its own separately admitted quiet run.

codegen-comparison.json confirms the ordinary constructor is back to 1972 bytes
and the shared counted-allocation helper no longer has an out-of-line symbol.
parse_string_value remains 1004 bytes with the reference's 96-byte stack frame.
