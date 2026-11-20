### GC ratchet: a bad cell now costs one cell, and the baseline is re-pinned at current `main` (#7554)

`gc-ratchet` had not been green on `main` since **2026-08-01T05:39Z** — 179
consecutive red runs. #7554 diagnosed one episode of that (the job dying in
preflight, fixed by #7557); the rest was the gate staying red against a
`0.5.1280` artifact that no longer described the collector. Either way it
produced no actionable verdict for a week, and #7594 and #7596 both had to
substitute hand-run both-arms A/Bs.

**Fail open per cell, fail closed on the verdict.** Artifact validation aborted
on the first defect, and it runs *before* the measurement step, so one cell —
`12_large_live_set.heap_used_bytes`, spread 6,768 bytes — meant none of the
twelve probes executed on any branch for three days. That blast radius was never
chosen; it was inherited from raising an exception. Defects now carry a scope:
`artifact` (unreadable or tampered) stays fatal and stays in preflight, while
`probe` and `cell` defects demote their subject out of the gating family and are
reported as failures. `check` therefore still measures everything, still
evaluates the rest of the matrix, and still names a regression elsewhere, while
the defect itself keeps the job red. CI preflight runs `validate --scope
structural`; `assemble` is unchanged and still refuses to *pin* any defect. A
test asserts that every defect `structural` waves through is one `check` then
fails on, so the flag cannot decay into suppression.

**Baseline re-pinned** at `main` `26b9c9d59` (0.5.1346) on `perry-macos` — the
same host and toolchain as the 2026-08-05 pin. All 12 probes oracle-pass;
`heap_used_bytes` spread 0 on eleven. Per-cell attribution lives in the
artifact's own `notes`:

- `03_cross_gen_writes` / `04_dead_after_deep_stack` shedding 40–99.8% of copy
  and promote work is #7594 + #7596 doing what they said. Recorded caveat: `03`'s
  `promoted_*` now pin at **0**, where the allowance floor and the liveness
  assertion both go quiet, so that cell no longer carries signal.
- `02_survivor_promotion` +2.77% and `05_closure_capture` +16.44% retention are
  conservative-scan false roots, not retention. `classify` gives `05` precise
  **5,329,880** — byte-identical to #7571's figure at both ends of its window —
  and `02` precise **9,416,632**, *below* what the old baseline recorded. That is
  #7559's answer, reproduced on the pinned host rather than assumed.
- **Flagged, unexplained:** `12_large_live_set.wall_ms` 3,056 → 3,471 ms
  (+13.58%), two non-overlapping 7-sample clusters on one host, while two other
  probes got 9.6% and 28.4% faster. #7596 reported −7.4% on that cell, so by its
  own evidence this is not #7596. `pinned_host`-only, so it does not block CI.
- **Did not reproduce:** #7596's accepted `12_large_live_set.heap_total_bytes`
  +36% reads 110,100,480 → 110,100,480 (+0.00%) under the harness protocol, so
  nothing was re-pinned for it.

**Two findings about the instruments themselves.** `PERRY_GEN_GC_EVACUATE=0`
moves **zero cells across all twelve probes** — the knob is inert for this suite,
so "passes with evacuation policy disabled" has never been evidence about it
(the #6942/#7024 pattern). And `gc-ratchet` is **not in branch protection's
required contexts**, so all 179 red runs blocked nothing; promoting it is
admin-only and should follow its first green `main` run, not precede it.
