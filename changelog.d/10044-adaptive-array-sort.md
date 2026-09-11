Speed up generic comparator sorting and shared dynamic operations. Adaptive
sorting over rooted indices reduces comparisons and repeated GC bookkeeping;
compact property caches, numeric guards, primitive-string comparisons, and
closure dispatch reduce comparator overhead without recognizing comparator
bodies. Collection remains enabled, with regression coverage for actual
relocation during comparison, collection getters, and allocating write-back.

On the recorded M1 Max run, the original 100,000-element scriptc #289 sort
measures 0.61 ms versus Node 1.68 ms (baseline Perry 90.97 ms).
Perry leads 24/25 measured cases. The PR includes raw samples, matching-build
hashes, full-output verification, semantic regressions, and a reproducible
harness in `benchmarks/array-sort/`. Results are local to this shared host.
