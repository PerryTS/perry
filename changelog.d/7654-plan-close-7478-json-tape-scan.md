**docs(engine-plan): #7478 — the JSON tape's scan penalty is closed, re-measured at v0.5.1370 (PR #7654).**

`docs/engine-plan.md`'s item 2 still described the JSON lazy tape as costing
**2.3× the direct parser on a full array scan**, with the v0.5.1299 sweep's
`field_access` inversion (optimized 2984 ms against idiomatic 1350 ms) as the
open problem. Every step of the issue's roadmap had shipped since — #7483
(DirectParser float parity, the step-1 blocker), #7499 (reparse-on-materialize),
#7537 (early batch-flip trigger), #7539 (tape out of the old generation), plus
#7546 for the wrong-JSON defect the work surfaced — but nobody had re-run the
decomposition the issue was written around.

Re-measured on the pinned quiet mini, 11 interleaved rounds, 176/176 samples,
every arm's checksum byte-identical to node 26.5.1:

| phase | tape on: issue → now | tape off: issue → now |
|---|--:|--:|
| parse only | 210 → **160 ms** | 1220 → 1196 ms |
| parse + full scan | 3030 → **1233 ms** | 1287 → 1224 ms |
| parse + stringify | 254 → **171 ms** | 1756 → 1449 ms |
| field_access | 2981 → **1721 ms** | — → 1480 ms |

The headline is gone: a full scan was 2.3× the direct parser and is now
**1.01×** (1233 vs 1224), while `roundtrip` — the path that must not pay for it
— improved 254 → 171 ms.

**Why it closes rather than continues.** Measuring all four `PERRY_JSON_TAPE` ×
`PERRY_GEN_GC` combinations instead of the two-switch `idiomatic` arm the
issue's floor came from, `field_access` decomposes additively: the tape costs
~200 ms whichever collector runs (241 under gen-GC, 195 under mark-sweep) and
the collector costs ~560 ms whether or not the tape exists (588 with, 542
without). The issue had documented these as an *interaction* — scan σ 214.9
under gen-GC against 8.8 under mark-sweep for the identical tape — and that
interaction is what #7539 removed. The ~200 ms is the tape *build* (parse-only
is 160 ms), structural and already named in #7537: the build is purely additive
whenever the whole tree ends up materialized anyway, and nothing can predict
scan-shaped access before the parse. The remaining ~560 ms is a
generational-collector term the tape-off arm carries identically, so it is
tracked with the GC work rather than as tape policy.

**Method, recorded in the doc.** The "re-measure before scoping" warning goes
from three instances to four, and gains a second failure mode this ticket
demonstrates: **a stale floor is as misleading as a stale headline.** #7478's
acceptance bar was "materially under the 1350 ms `idiomatic` row", but
`idiomatic` is a measurement rather than a constant and had itself moved to
938 ms. Working the ticket as written would have chased a headline that four
merges had already fixed, and then declared failure against a bar that no
longer existed. When acceptance is "beat arm X", re-measure arm X in the same
run.

**Verification.** Both reported runs are on a host gated to `rustc == 0` *and*
1-minute load < 1.5 held across two consecutive checks, and they agree
independently; a first attempt was discarded when another agent's build started
mid-run (load 1.86 → 8.57, every σ 50–600). The harness fails rather than
blanks on a cell that produced no sample or no checksum and asserts its
expected sample count, so a pass cannot be confused with "it never ran".
Subject liveness is asserted both ways: the tape is demonstrably engaged
(tape-on/tape-off differ 7.5× on parse-only and 8.5× on roundtrip), and
`cargo test -p perry-runtime --lib json_tape` passes 24/24 including the two
cases that distinguish *which producer ran* via `reparse_materializations()`.
Binaries built `-p perry -p perry-runtime-static -p perry-stdlib-static` with
`PERRY_RUNTIME_DIR` pinned, the archive mtime asserted to have moved, and
`PERRY_NO_AUTO_OPTIMIZE=1` on every compile.

Documentation only — no runtime or codegen change.
