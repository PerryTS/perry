### Fixed

- **gc:** the page-class table now remembers that a 1 MiB class holds no
  registered range, so a candidate address in no GC block is answered from the
  table instead of a failing hash lookup in the authoritative
  `PageGenerationMap` (#9852).

  After the direct-indexed table (#9853) took classification misses from
  18.5 % to ~6 % of lookups, what remained was dominated by addresses the map
  cannot answer either: conservative-scan candidates, interior pointers and
  non-heap words that every classification path re-asks about millions of
  times per turn. Counted on the compiled claude-code TUI, 3300-char reply,
  three runs: **52.8-62.1 % of every remaining miss** is a class the map does
  not hold at all, at 41.5-66.2 % over two 400-char runs.

  A negative entry is an ordinary table entry carrying the current epoch, so
  the registration that would make it wrong is the same event that already
  invalidates every positive entry — no new invalidation obligation. It is
  written **only** where `pages.get(&key)` is itself `None`; a class that holds
  a range the address happens to miss is a different case (16-29 % of the same
  population) and caching "absent" for it would answer wrongly for the
  addresses the class really covers. `PERRY_GC_PAGE_CLASS_NEGATIVE=0` restores
  the previous behaviour in the same binary.
