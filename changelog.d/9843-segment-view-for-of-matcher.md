Compile-time matcher for the `Intl.Segmenter` `for…of` loop, with the counter
that decides whether it fires.

`for (let {segment: O} of X.segment(q))` is where claude-code spends most of a
turn: the allocation census ranks it 1/2/3 by count (172,032 segment records
plus 247,808 substrings per 400-character reply, 58 % of the top-30 allocation
count), and a `sample` puts 60–85 % of active main-thread CPU inside it, under
ink's `wrapText`. The loop reads one code point per grapheme and retains
nothing.

`collectors/segview.rs` joins `escape_news` / `escape_arrays` /
`escape_objects` as the family's fourth member. It proves one thing: the
segment RECORD never escapes, because every use of the synthetic
`__destruct_N` binding is one of the destructuring field reads the loop head
itself emits. Uses of the segment STRING are classified and counted but never
gate the proof — a use no view entry point can answer is served by
materialising the substring once, which is what the loop costs today.

The escape proof is taken with `perry_hir::collect_local_refs_stmt`, whose
descent bottoms out in the walker the compiler forces to be exhaustive, so a
new HIR variant embedding a `LocalGet` cannot silently hide a use of the
record.

No lowering yet: the fact is populated and unread until the runtime's
segment-view entry points exist. `PERRY_SEGVIEW_DIAG=1` reports every site
examined, its verdict and its per-use tally at the HIR-trace point, and is
excluded from the build-level cache so a report of zero is a measured zero
rather than a build that never lowered HIR.
