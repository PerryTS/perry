Empty-object stringify can return its inline result before creating a field
plan or a temporary runtime handle. It refreshes the prototype verdict on each
call and requires the no-allocation probe; a cold lookup declines to the rooted
general serializer. Nonempty objects retain their input root before lookup and
before output allocation. No GC production policy changes are included.

135 runtime JSON-filter tests pass, including a forced-moving cold-empty-input
fallback and the prior nonempty first-lookup witness. 1000 warm leaf calls show
no managed growth or collection. All 32 compiled Node comparisons and 22
moving-GC stress runs pass. The new compiled witness also matches the preceding
runtime: prototype methods/getters, deletion, explicit/null prototypes, hidden
toJSON, replacers, spacing, mutation and retained output remain observable.

The release passes a full qualified matrix (152 verifications, 456 timing and
432 memory trials, 32 clean external observations) and 210 paired trials
(15 cases, 13 clean observations). Both load/competing-process gates pass.

Paired empty-object stringify improves 11.08% against the record release and
11.46% against the validation-reuse build. Against the record release, 1 MB
record stringify improves 4.00%, 8 MB improves 3.53%, and 20 MB improves 1.27%.
The inventory remains 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS targets.

The paired check confirms escaped parsing +0.65% and 1 MB object-root parsing
+0.31% versus the record release. Numeric parsing's +1.43% full-matrix delta
does not reproduce: the paired ranges overlap with a -0.32% median change.
This does not settle its older regression versus Unicode. No-regression and
all-row acceptance remain open. All rows and ranges are retained in tables.md,
parity.md and recheck-empty-object-leaf/summary.json.
