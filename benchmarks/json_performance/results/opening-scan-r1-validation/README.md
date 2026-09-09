# Opening-byte scan R1 validation

Source 29236271f61a6cfb24de810ff4fe1f884c774c5e, package 0.5.1530. The only
runtime change from corrected R2 is parser_depth_blocks.rs; no source-length
shortcut, collector policy or allocation-placement change is included.

All 285 JSON tests pass single-threaded in release mode, including the two new
scanner boundary/byte/poisoned-surrounding tests. Matched compiler/runtime-static/
stdlib-static build passes (6m59s). Raw-handle, address, root, format and file-size
gates pass. Frozen source and newly linked worker hashes are in provenance.json.

Compiled output and retained lifetimes match Node under ordinary, scheduled /
protected and full GC. Scan witnesses 1323 protected copying minors. One thousand
changing Unicode parses witness 26 ordinary copying minors. Malloc-sweep cadence
remains 39,40,40,40 in candidate and corrected R2. All numeric RESULT fields are
finite; all 14 compiled escaped-record comparisons match Node and 12 fail against
the immutable unfixed-main negative control. These are correctness witnesses,
not performance timings.

Code inspection confirms parse_string_value remains 976 bytes with a 96-byte
frame, and the ordinary string constructor remains 1972 bytes. The exact local
validation driver is preserved as validate-recorded.txt; it is an archival
build procedure with local prerequisites rather than a portable entry point.
