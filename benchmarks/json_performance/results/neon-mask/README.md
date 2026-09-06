Rejected experiment; this scanner is not in the current implementation.

Replacing the ARM first-hit lane search with two register-word extracts and
trailing-zero counts passed 136 runtime JSON tests, 32 compiled Node output
comparisons, 22 moving-GC runs and 38,207,488 standalone byte-pattern checks.
The full measurement window was also qualified: 152 output verifications,
456 timing trials and 432 memory trials, with 32 clean external observations
and passing load/competing-process gates. Correctness and measurement quality
did not make the implementation faster.

Against the qualified empty-object release, escaped parsing used 50.63% more
CPU; record-array parsing used 9.51% more at 16 KB, 8.42% at 1 MB and 5.89%
at 8 MB. Null and inline-string parsing also regressed. All rows, including
the occasional gains, remain in tables.md and the raw trial files. No paired
follow-up was required to reject these large full-matrix regressions.

The experiment reduced build_tape_into's disassembled span by 18.2%. However,
LLVM already eliminated the original source-level mask store: the old binary
tested register lanes and returned early for common positions. The replacement
appears to lengthen those paths. Smaller generated code was not a speedup.

source.patch.gz applies to f7bc8848770d5b03422478de6e44384feeae6015 and was
verified to reproduce all 40 recorded source hashes. The worktree was restored
to all 40 hashes of that qualified empty-object release. The rejected linked
worker and archives remain separate from the current release artifacts.
