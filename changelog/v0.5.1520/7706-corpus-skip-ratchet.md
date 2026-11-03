### Fixed

- **`gc_root_dominance_corpus.sh` tolerated 41 sources failing to compile and still exited 0.** The compile floor was the constant `MIN_COMPILED=90`, hand-synced to the `PATTERNS` list by a comment reading *"Keep this list in sync with MIN_COMPILED below when you add a prefix."* The comment drifted: `PATTERNS` grew to discover **131** sources while the floor stayed at 90. A run in which 41 sources failed to compile printed the failures to a log and exited 0.

  That is CLAUDE.md's hazard 4 — "the gate runs but its subject never did" — with a twist that makes it worse than a skipped test: a source that fails to compile emits **no IR**, and IR that was never emitted reads to the dominance checker exactly like IR with no violations in it. The absence is indistinguishable from a pass at every downstream floor in the script.

  A floor expressed as an absolute count cannot track a corpus that grows. Both numbers are now ratchets against the corpus **as discovered**, and both are checked in **both directions**:

  - `MIN_SOURCES` (131) — how many files `PATTERNS` must still match. Falls only when sources are deleted or renamed. That is the "corpus shrank" finding, which the old single floor conflated with "sources failed to compile" — two different failures with two different fixes, now reported separately.
  - `MAX_SKIPPED` (0) — how many discovered sources may fail to compile. Zero is the measured truth on both lowerings, not an aspiration: shadow and native each report `131/131 sources compiled, 0 skipped` as of v0.5.1402. Over budget is a regression; **under** budget also fails, naming the number to write down, so the budget can never drift above reality the way `MIN_COMPILED` did.

  The derived compile floor (`MIN_SOURCES - MAX_SKIPPED`) removes the hand-sync entirely — there is nothing left to keep in step with `PATTERNS`.

  All three arms are sabotage-verified rather than assumed: planting an uncompilable `test_gap_gc_*.ts` gives `1 of 132 sources failed to compile (budget: 0)`, exit 1; `MAX_SKIPPED=3` against a reality of 0 gives *"the budget is stale and would absorb the next real failure silently"*, exit 1; `MIN_SOURCES=999` gives *"PATTERNS matched only 131 sources"*, exit 1.
