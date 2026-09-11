# R42: keep object parsing out of the value dispatcher

Hypothesis: the SOURCE_LENGTH=true specialization introduced in R37 lets LLVM inline the large object parser into parse_value. The R39 records_object_8m profiles show a roughly 7 KiB value-dispatch function, unlike R26's outlined object parser. Forcing just parse_object_untyped out of line may recover ordinary record and wide-object performance while retaining the dominant-string proof and R41 packed escaping.

Change only the inlining policy of parse_object_untyped. No parsing algorithm, GC boundary, roots, admission, thresholds or caps change. No test that merely asserts the annotation is added. Run existing serial JSON and string runtime tests, full lint, a fresh production build of all three packages, fresh complete output checks and native/shadow root comparisons. Inspect generated machine code to establish whether the intended boundary changed.

Measure the known wide/records/heterogeneous regressions first. Keep R41 as an independently measured immediate-parent control and R26 as the interleaved historical reference. Preserve all failed windows and noise exclusions. Only broaden to fresh rotating inputs, Korean strings, all 50 rows and stringify options if the target change merits continuing. Do not claim current-main or no-regression status from a narrow screen.
