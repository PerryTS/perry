### GC: long-lived cohorts are copied once, not twice — the promote-on-first-copy seed (#7598)

#7592's remainder. On promote-heavy workloads every long-lived object was copied
**twice** — Eden → survivor by one copying minor, survivor → old by the next.
`json_pipeline` at 500k: cycle 3 copied 280,997,080 bytes into the survivor
space, cycle 4 promoted the same bytes out of it.

`gc/tenuring.rs` already computed the right condition. The survival-rate lock
keys on `prev_copied`, so it needs a **previous copying minor** to have filled
the survivor space — it engaged for cycle 4, which is precisely why the waste
was confined to cycle 3. The obstacle was latency, not blindness.

**Every collection that reaches the mark-sweep path already walks every Eden
header and classifies it live or dead.** Those are exactly the two blind spots
of `retune_after_scavenge`, which only copying minors feed — a full mark-sweep,
or a non-copying minor fallback. So the census is read there, and when the
surviving Eden cohort exceeds the desired survivor occupancy (the module's
*existing* occupancy rule, read from a different place) **and** ≥90% of the
classified Eden bytes were live, the existing `PROMOTE_LOCK` engages and the
next copying minor *enters* at S=1. Exit stays the influx-based unlock, so the
seed adds no new oscillation path.

**Why the signal is not a fixed point of the policy reading it.** This issue
produced three self-referential signals before this one: `promoted_bytes` is
zero by construction at S=4; #7596's first nursery cap gated from-space
occupancy on a total that included from-space; #7594's handoff scheduled a
non-moving full to relieve pressure only a moving cycle can relieve. The Eden
live/dead split cannot have that structure — it is produced by the mark-sweep's
own arena walk, whose marks come from reachability, and neither the mark phase
nor the sweep reads `tenuring_survivals()`. The threshold is consulted in
exactly one place, `copying.rs`'s per-object move, which this path does not run.
Measured while S was still 4: `eden_live_bytes=279,964,968 eden_dead_bytes=896
live_pct=99 seeds=true`.

**Determinism (#7432)** is preserved by construction: written at the end of a
completed sweep, read at the entry of a later copying minor, which snapshots it
once. Two callsite exclusions keep the *input* sound as well — budgeted cycles
(allocate-black marks every mid-cycle birth, so a churn Eden would read ~100%
live) and cycles that ran the conservative native-stack scan (its retention
varies run to run, so a policy fed from it would make the gated copy/promote
counters non-deterministic).

**Measured**, `json_pipeline`, output hash identical on every row:

| records | arm | collections | copied_bytes | promoted_bytes | bytes moved |
|--:|---|--:|--:|--:|--:|
| 200k | before | 4 | 113,227,216 | 114,275,776 | 227,502,992 |
| 200k | after | 3 | **0** | 113,227,216 | **113,227,216** |
| 500k | before | 4 | 280,997,080 | 282,045,656 | 563,042,736 |
| 500k | after | 3 | **0** | 280,997,080 | **280,997,080** |

Bytes moved **0.498× / 0.499×** — halved, which is the signature that separates
this from a cadence change. The per-cycle trace makes that explicit: cycles 1–3
are unchanged in kind, trigger, `old_before` and `eden_live` — cycle 3 receives
the *same 280,997,080 bytes to the byte* and merely sends them to old-gen
instead of the survivor space. The cycle that disappears is the old cycle 4,
whose own Eden influx was 1.0 MB and whose entire content was the second copy.

On the pinned quiet host (`perry-macos`, 5 interleaved reps, `cmp`-identical
output): 200k **1.85 s → 1.44 s (−22.2%)**, peak RSS 608.7 MB → 485.7 MB
(**−20.2%**); 500k **5.12 s → 3.86 s (−24.6%)**, peak RSS 1,404.5 MB →
1,109.8 MB (**−21.0%**).

**RSS goes down, not up**, which is the opposite of the design note's
expectation. Promoting earlier does raise the old-gen high-water mark, but it
removes a larger term: at S=4 the 268 MB cohort exists twice at once at the
peak, as Eden from-space plus survivor to-space. This is the first change in
this campaign whose wall-time win is not traded against RSS.

**gc-ratchet (`--check`, `pinned_host`, on the #7609 baseline):** OK. Every
semantic counter on all 12 probes is **bit-identical** to a `main`-arm run
measured in the same session on the same host — the only differing cells are
`rss_bytes`/`peak_rss_bytes`/`wall_ms`, all inside band and all moving in both
arms. The seed does not fire on any probe, and the diagnostic proves that is a
*refusal*, not an absence: `12_large_live_set` prints `live_pct=36 seeds=false`.
`12_large_live_set.wall_ms` — #7610's flagged cell — reads −13.11% on the
`main` arm and −13.14% on this one, so this change does not move it.
