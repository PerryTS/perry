**The thread-local policy gate is green again, and now measures what it claims to.**
`scripts/check_thread_locals.py` was failing on `main`, which blocked
`tls-budget`'s documented promotion to a required context — a gate that runs,
reports failure, and cannot block anything. Two of the eight raw `thread_local!`
blocks were real, and they were not the ones reported: `array/indexing.rs`,
`map.rs`, `set.rs` and `registry_latch_probes.rs` are all `#[cfg(test)]` and do
not exist in a shipping build, while `gc/schedule.rs`'s `SAFEPOINT_COUNTER`
(every safepoint) and `SCHEDULE_NEXT_CANDIDATE_BYTES` (every poll) were live and
paying `_tlv_get_addr` on Darwin. Those two now use
`crate::perry_thread_local!`. The scan no longer counts `#[cfg(test)]`
declarations at all — recording one as "cold" records the wrong fact, since it
is not cold but absent — covering the attribute directly above a block, an
inline `#[cfg(test)] mod`, and a whole file declared `#[cfg(test)] mod <stem>;`
(closed transitively, so `gc/tests/mod.rs` carries its subtree). Because an
over-broad exclusion would make the gate pass *by seeing less*, `--self-test`
now checks each shape in both directions — gated is skipped, and removing the
gate makes the same declaration fail again — proving six rejection directions
instead of four. The allowlist regeneration is provably one-way (101 → 91
entries, 157 → 129 blocks, nothing added and no count increased), which also
retires three already-stale entries: `gc/zeal.rs` from #7741's removal of
`PERRY_GC_ZEAL`, plus `arena/quarantine.rs` and `gc/oldgen_defrag.rs`. (#7814)
