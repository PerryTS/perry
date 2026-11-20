### Gates: repair the gc-root-dominance probe whose premise rotted on a file move

`gc_root_dominance_check.py`'s `_probe_old_defrag_off_by_default` read
`crates/perry-runtime/src/gc/oldgen.rs` to confirm old-page defrag is still
opt-in. #7443 moved that code to `gc/oldgen_defrag.rs`, so the probe stopped
finding `PERRY_GC_OLD_DEFRAG` and began reporting
*"old-page defrag may now be unconditional"* — on a tree where it is, in fact,
still gated exactly as before.

That is worse than a red job. This probe's failure does not merely fail the
run: by the checker's own contract it declares **the reports it was
suppressing to be real again**, so a file rename silently converts a passing
audit into a wall of false positives in the gate family that the whole #7341
rooting campaign depends on.

The probe now searches a list of candidate sources and reports which one it
validated, so the next split costs a one-line addition instead of a rotted
premise. Its failure message names the list. Sabotage-verified both ways:
deleting the `old_page_defrag_enabled()` short-circuit still makes it refuse,
so the repair did not turn it into a rubber stamp.
