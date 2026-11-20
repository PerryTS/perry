### Fixed

- **Old-page defragmentation is opt-in again** (`PERRY_GC_OLD_DEFRAG=1`). #7913's rewrite-contract work — the `PARSE_KEY_RING`/diagnostics/perf-hooks rewrite coverage, class-key locals as precise mutable roots, all-or-nothing source-block evacuation, and the tightened runtime-holder policy — **all stays**; only the default flip is reverted.

  #7876's own acceptance criteria required "a dependency-scale stress corpus clean" before re-enabling, and no such corpus exists. The structural reason it cannot exist yet: selection needs `dead_bytes >= live_bytes` on an old page, i.e. promote-then-die at scale, and no program in the 19-benchmark corpus can produce a candidate page — the `retain` family survives at 999–1000‰ (old pages ~fully live) and the `churn` family promotes almost nothing. Default-on therefore bought no benefit signal and no regression signal while inheriting the full old-address rewrite surface. Every GC gate was also still queued and unexecuted when it merged.

  An unrecognised value is now **OFF** rather than ON, so a typo cannot silently enable old-generation relocation.

  When a fragmentation workload exists that can exercise this, the losing arm gets deleted rather than left standing.

- **The OFF state has behavioural coverage for the first time.** `select_old_page_defrag_pages_from_snapshot` does not consult the knob — the gate lives only in `select_old_page_defrag_pages` — so every pre-existing selection test bypassed the switch entirely, and the thread-local override was only ever set to `Some(true)`. Adds `OldDefragTestDisable` and a test asserting the disabled path short-circuits **before** the O(old pages) page-meta snapshot, pinning the gate's placement and not merely its effect.

  The test asserts the snapshot call counter rather than the returned selection, with a load-bearing positive control: its first version asserted only "disabled returns nothing", which is vacuous in a process with no eligible old pages and passes identically against a kill switch that does nothing. Sabotage-verified — deleting the early-out turns it red while the value-mapping test stays green.
