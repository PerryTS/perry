Local 20 MB record-stringify sampling diagnostic, current data-record worker.

The worker and sampler both exited successfully; command and binary SHA-256 are
in metadata.json. This is an Apple M1 Max development host under heavy load,
not a qualified CPU/RSS comparison. Sampling counts cannot establish an exact
speedup or replace the shared-M1 acceptance matrix.

Of 2,167 main-thread samples, 1,452 enter the GC from the benchmark loop's
moving-minor safepoint (about 67%). Another 696 enter JSON.stringify; 37 of
those are in the final output copy. The sample therefore supports investigating
GC as a major contributor to the 20 MB result, not treating buffer copies as
the main observed cost. GC production code and policy remain unchanged here.

Within the JSON traversal, primitive-array emission still repeats header,
named-property and element validation already done by the record preflight.
The next JSON-only experiment can eliminate this repeated validation while
keeping the complete preflight and its callback-free interval intact.
