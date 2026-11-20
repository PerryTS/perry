### Fixed

- **`gc-ratchet` had measured nothing since 2026-08-05 (#7554).**
  `test_pinned_artifact_retention_is_deterministic` failed in the *"Harness unit
  tests and artifact validation"* step, which runs **before** the measurement
  step, so all twelve probes were skipped on every branch — and `gc-ratchet` is
  not a required context, so the red blocked nothing. Roughly fifteen
  GC-affecting changes landed on 2026-08-06 with no standing gate over them.

  The assertion was right: `12_large_live_set` retention really had stopped
  being bit-identical, and `tolerances.json` justifies gating `heap_used_bytes`
  on an observed spread of 0.000%, explicitly as anti-brittleness margin rather
  than a noise allowance. What was missing was the lever the assertion's own
  message points at. `tolerances.json` is keyed per metric per profile, so
  "take this metric out of the gating family for this probe" could only be said
  by turning `heap_used_bytes` gating off for all twelve.

  `tolerances.json` gains a `probe_overrides` section that removes one
  `(probe, metric)` cell from the gating family. A band expresses a machine
  class's noise floor, which is per profile; whether a metric is deterministic
  enough to gate at all is a property of the workload, which is per probe.
  Because an exclusion is a hole in a gate, every property of the mechanism is
  a refusal: it may only set `gating` to `false`, never back to `true`; it never
  touches the band, so an excluded cell is still measured, still compared, and
  still printed as `drift (informational)`, with its full reason printed under
  the table on every run; its evidence is checked rather than stored (at least
  21 runs — the same number every band in the file rests on — and a spread that
  is actually non-zero, so a metric cannot be excluded without having been shown
  ungateable); an override that matches no probe **fails**, the rule
  `scripts/gc_root_dominance_allowlist.json` already carries; and an override
  set that covers every probe for a metric fails, because assembled one cell at
  a time that is a profile-level `"gating": false` with nowhere to read the
  reason.

  The bit-identity rule itself moves from the unit tests into
  `validate_artifact`, so an artifact carrying a non-deterministic gating cell
  can no longer be *pinned*. Had it lived there on 2026-08-05, the re-pin would
  have failed on the maintainer's machine at the moment the judgement was made,
  instead of silently wedging CI afterwards.

- **A probe that stopped collecting was reported as passing.** `check` now fails
  a probe whose current run reports `minor_cycles == 0` or `copied_objects == 0`
  where the baseline reports more, instead of leaving that to the tolerance
  arithmetic. The arithmetic could not catch it: six of the twelve probes pin
  `minor_cycles` at 1 and the allowance floor is also 1, so a collapse from 1 to
  0 is `delta == -allowance` and scored `ok`. A collector that stops running
  copying minors is the largest regression this ratchet exists to catch, and it
  was the one shape the gate could not see — CLAUDE.md's fourth failure mode
  inside the gate built to close it.

### Measured

- **The probes run again.** Full `measure` + `check`, twelve probes each, on two
  machine classes: the pinned quiet host (`perry-macos`, M1 mini, load 2.2,
  `pinned_host` profile) and a MacBook Pro under load 21 (`shared_ci`). All
  twenty-four probe runs compiled, ran, and passed their Node-oracle diff, and
  retention and the GC counters reproduced bit-for-bit across the two hosts.

  Both arms report the same ten gating breaches, invisible since 2026-08-05.
  Most read as improvements wearing a two-sided band — `03_cross_gen_writes` and
  `04_dead_after_deep_stack` shed 40–95% of their copy/promote work while their
  retention *fell* 49% and 22%, so objects that used to be copied and tenured
  are now recognised as dead. Two are not: `05_closure_capture` retains
  **+16.44%** with `copied_objects`, `copied_bytes`, `promoted_*` and
  `freed_bytes` all at `+0.00%` — the same collector work, more retained — and
  `02_survivor_promotion` is +2.77% on the same shape. This change deliberately
  does **not** re-pin: re-pinning to turn a red gate green is what the artifact
  exists to prevent, and those two want a look before they are accepted.

- **`12_large_live_set`'s non-determinism is the conservative stack scan, not
  the collector's steady state.** Every probe reads `process.memoryUsage()`
  after an explicit `gc()`, and an explicit `gc()` runs a full mark-sweep with a
  forced conservative stack scan (`[gc-scan-fallback] site=manual_collect
  automatic=false`, printed on every run). Such a scan retains whatever the
  native stack looks like a pointer to, and stack residue differs run to run.
  Diffing two disagreeing `PERRY_GC_DIAG` traces shows it exactly: the minors,
  the tenuring decisions, the step cycles and every copy/promote counter match,
  and the sole difference is the *last* collection's `freed_bytes`. Under
  `PERRY_CONSERVATIVE_STACK_SCAN=off` the probe reports **51,668,688 bytes on 8
  consecutive runs, bit-identical**, against 59,943,080–59,952,824 by default.
  So the variance is entirely false roots, and the conservative scan is
  systematically retaining **8.28 MB — 16% of this probe's reported retention**.
  The eleven small probes stay bit-identical because their live sets are one to
  two orders of magnitude smaller, so a stale stack word is far less likely to
  alias a plausible heap address.

- **No 29% retention win.** The pinned artifact was captured on
  `perry-macos.fritz.box` — the *same* Mac mini as the 21-run experiment in
  #7554, so there was no host difference to control for. Measured with the
  harness's own protocol, `12_large_live_set.heap_used_bytes` is 59,943,824 at
  the pinned `5e236e6e2` (2026-08-05), 59,943,224–59,949,920 at `52f7dae1f` (the
  commit the 42.6 MB reading was taken at), and 59,943,896 at current `main` on
  the pinned host. Retention on that probe has not moved across the whole
  2026-08-06 batch. `PERRY_GC_DIAG=1` does not change it — the harness's
  traced/untraced split still holds — and neither does the auto-optimizer.
