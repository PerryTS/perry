# Opening scan R3: correctness and codegen

Version 0.5.1530, source change d91466d4f: the initial 16-byte positive check
remains inline, and the wide opening-search tail is a separate function.
All 285 release JSON tests pass single-threaded (1.58s); frozen source hashes
match. The matched compiler/runtime-static/stdlib-static build passes in 5m19s.
Raw-handle, raw-store, address-class, root-holder, formatting and size gates pass.

The compiled scan performs 1323 protected copying minors and moves 208531
objects while matching Node. Retained outputs also match through actual moves
and full-GC mode. All fourteen escaped-record cases match Node, with twelve
failures in the unfixed-main negative control; emitted RESULT values are finite.
Stringify malloc-trigger cadence stays 39,40,40,40 in candidate and reference.

The generated depth scanner is 3676 bytes (reference 3768; scanner R2 3904).
The outlined search tail is 284 bytes. The depth frame remains 80 bytes; the
tail has a 16-byte frame. Shared parse entry remains 156 bytes/frame16,
parse_slow 5416 bytes/frame496, string parser 976 bytes/frame96 and ordinary
string constructor 1972 bytes/frame96. Exact assembly is archived.

Timing/RSS acceptance is pending. No global GC policy, allocation placement or
construction behavior changes are included. The exact validation/inspection
drivers are historical records with local paths, not portable replay tools.
