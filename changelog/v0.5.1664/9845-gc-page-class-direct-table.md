**The page-generation cache becomes a direct-indexed table over the arena's
1 MiB address classes, so a classification is a bounds compare and one load
instead of a four-way probe that missed one call in five.**

`classify_heap_generation` and `classify_heap_space_in_range` sit under three
callers with no cheaper predicate of their own. The write barrier's
`remembered_child_needs_tracking` runs **35,871,391 times per turn** on the
compiled claude-code TUI and **95.23 %** of those take its cheapest arm — one
cached classification and a compare — so there is no barrier predicate left to
fix: what remains after the predicate is already optimal is the classification
itself. `mark_addr` (233 of 760 `classify*` leaf samples) and the side-table
prunes pay the same cost.

The structure in front of the authoritative `PageGenerationMap` was a **4-way
round-robin set**. Measured with a dedicated counter on a 3300-char streaming
reply: **440 M lookups per turn at 20.0–21.6 % miss**, with **59.7–61.8 % of
misses on a key evicted within the last 64 evictions** — capacity, not conflict —
against a working set of **402–432 registered classes**. `ways_distinct_max` was
4, so every way was already in use and the shortfall is ~120x.

**Widening it was not an option, and the reason is on the record.** #7469
measured 16 ways as an **8.6 % regression** on the same row (0/7 pairs) for 1.5 %
fewer misses, and five further associativity changes measured flat. The rule
those produced — *associativity pays only when a miss is expensive* — says that a
miss which is just a hash lookup wants the cache to become **unnecessary**, not
larger.

It can be. The registered classes occupy a span of **1,018–1,021 classes at
~40 % density**, so a table over that span holds every one of them in **160 KB**
and answers with one bounds compare and one load. `PageGenerationMap` stays
authoritative and every miss falls through to it exactly as before; the change is
confined to `PageGenerationCacheSet` and its two callers.

Four things the measurement did not settle, each handled explicitly and each
pinned by a test that fails when its guard is removed — a wrong answer here is a
misclassified pointer, so none of them is left to inference:

* **The base moves per process** (`0x43daa2` vs `0x57e3c2` on two runs — ASLR).
  It is taken from the first insert, never compiled in.
* **The span can grow** (1,018 → 1,021 across two runs of one binary). An insert
  outside the table rebases it, up to a 16,384-class cap; past the cap the key is
  left uncached and falls through to the map rather than being mis-indexed.
* **The sizing is not obvious.** With base `first_key - S` and a table of `N`,
  the span covered is `min(S + 1, N - S)`, maximised at `S = N / 2`. The natural
  pairing `N = 4096, S = 1024` covers **1,025** classes — four above the measured
  span — while `S = N / 2` covers **2,048** for the same memory. A `const` assert
  now fails the build for any pairing covering less than twice the measured span.
* **A key match is not an address match.** A class can hold more than one range,
  so a hit still requires `range.contains(addr)`.

Invalidation is an epoch bump: O(1), and the same "clear everything" contract the
4-way set met by being reset wholesale. That contract matters more here, because
the table holds ~2,000 entries where the set held 4 — a missing invalidation the
old structure survived by luck would be a live misclassification — so all three
`PageGenerationMap` mutation sites were enumerated and each ends with an
unconditional `invalidate_generation_cache()`.

The arm is a plain `u8` field in the set's first cache line rather than the env
`OnceLock`: this path runs 440 M times per turn, and an acquire load on each
would have been charged to both arms of the A/B — hiding it in the comparison
that was meant to isolate it — while still being paid against main.
`PERRY_GC_PAGE_CLASS_TABLE=0` restores the 4-way set in the same binary, which is
how the numbers above and below were taken.
