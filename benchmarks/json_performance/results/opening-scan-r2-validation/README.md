# Opening scan R2: correctness and GC validation

Version 0.5.1530. All 285 release JSON unit tests passed single-threaded; every
frozen source hash was checked. The matched compiler/runtime-static/stdlib-static
release build passed in 5m34s. Raw-store, address-class, root-holder, formatting
and file-size gates passed. The prior unrelated box.rs dead-code warning is
retained in the exact build log.

Compiled scan/retained-output checks match Node in normal, scheduled moving-GC
and full-GC modes. The scan witness performed 1323 protected copying minors and
moved 208531 objects. Retained-output moving tests also performed actual moves.
All fourteen escaped-record comparisons match Node; the unfixed-main negative
control fails twelve. Every emitted RESULT field is finite. The original
worker objects were relinked against the newly matched runtime archive; all
hashes are in provenance.json. Stringify malloc-trigger cadence stays
39,40,40,40 in both candidate and corrected R2 reference.

Generated ARM64 code shrinks the shared js_json_parse entry from 1736 to 156
bytes, with its stack frame shrinking from 96 to 16 bytes (twelve registers
saved to two). Ordinary parse_slow shrinks from 7232 to 5416 bytes. The string
parser/constructor sizes and 96-byte frames remain unchanged at 976/1972 bytes.
The opening preflight is 3904 bytes, the same as scanner R1.

Timing/RSS acceptance is separate and pending. The exact archived validation
and inspection drivers are historical records with local paths; they are not
advertised as portable replay tools. Runtime tests and archived raw outputs,
source patch, checks, codegen and immutable provenance make their results
reviewable. No GC policy or collection-cadence change is included.
