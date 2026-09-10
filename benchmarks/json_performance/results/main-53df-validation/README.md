# Fresh main 53df build and compiled validation

The accepted JSON changes landed through train #10037 at
`53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530). A fresh fetch and complete
tree diff prove this main commit is byte-identical to final PR #10036 head
`6a5d2ba5e7e8ba287179b853a9386e000d486aa3`. The original PR was closed after the
train landed; its own null mergeCommit does not mean the work was lost.

The matching compiler/runtime-static/stdlib-static release build passed in 5m25s.
All 62 frozen source hashes and HEAD stayed unchanged. New original, rotating
and retained-lifetime workers were relinked against the new archive. Compiler,
HIR, transform and parser source trees match the earlier main eee reference,
supporting reuse of the same runtime-only benchmark objects. All object, binary,
archive and selected source hashes are recorded; old timings are not relabelled.

Runtime source matches the previously tested tape-depth R2 source df64624c8
exactly. The reference's 291 release JSON tests are archived separately; this
packet does not claim a fresh unit-test run. The new main build received fresh
compiled checks: protected scan moves 208531 objects through 1323 copying minors,
retained outputs survive normal/scheduled/full GC, Unicode triggers actual minor
collections, and recurring stringify malloc counts match the old main at
39,40,40,40. Fourteen escaped-record comparisons pass. A newly compiled depth
fixture matches Node in all nine tape/GC modes, with live moving/protected roots.

All measurement-host fixture bytes match the original and eight-input manifests;
the staged workers and Node26.5.1/Bun1.3.14 versions are verified. Complete
measurement windows are archived separately after final quiet admission.

This main runtime contains neither rejected canonical correction R4 nor R5.
Inherited canonicalization failures and the public freshness/Linux stack-test
CI issues remain separate limitations. Full performance parity is not claimed.

Run the archived `summarize_original.py`, `summarize_rotating.py` and both
`verify_checksums.py` scripts with their result directory as the argument.
Then run `python3 benchmarks/json_performance/results/main-53df-validation/summarize_main.py`
from the repository root to regenerate the headline report from tracked data.
Large diagnostic logs use deterministic gzip; `compressed-logs.json` records
both uncompressed and compressed hashes. Read them with `gzip -cd FILE.stderr.gz`.
The untouched raw copies remain in the local build archive.
