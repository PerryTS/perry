**GC performance/RSS ratchet (Task 0 of the GC architecture campaign):** pin the
current evacuating minor collector's observable behaviour and gate on it, before
the shadow-stack-root removal work starts. Measurement and infrastructure only —
no compiler or runtime behaviour changes.

- **New artifact** `benchmarks/gc_ratchet/baseline/gc-ratchet-v1.json`, captured
  on the pinned quiet host (Apple M1, 8 cores, 8 GB, macOS 26.5.1) at load 1.19
  with CPU-active confirmed at 5.9%, recording per-probe distributions, the host
  and load average, toolchain versions, and SHA-256 content hashes of `perry`,
  `libperry_runtime.a` and `libperry_stdlib.a`. **Distinct from the public
  Node/Bun baseline** in `benchmarks/results/public-node-bun-v1.json`, which is
  untouched: different directory, different workflow, different gate, and the
  distinction is restated in the artifact, the README and every file header.
- **Eight probes** (`benchmarks/gc_ratchet/probes/`) covering nursery churn with
  a zero live set, survivor aging and promotion, old-to-young stores, dead
  objects under a deep stack high-water mark, closure environments, heap strings,
  array element-storage growth, and Map/Set side tables. Each probe's stdout is
  diffed byte-for-byte against the Node pinned in `.node-version`; all eight
  pass. Every probe parks its allocations in a heap container before dropping
  them — an earlier draft allocated into non-escaping locals, LLVM scalar-
  replaced them, and the probe ran in 10 ms with zero collections while looking
  healthy.
- **Four metric families, separated by noise floor.** Retention
  (`heap_used_bytes`, `heap_total_bytes`, read after an explicit full `gc()`) and
  the evacuating minor's own accounting (`minor_cycles`, `step_cycles`,
  `copied_objects`, `copied_bytes`, `promoted_objects`, `promoted_bytes`,
  `freed_bytes`, parsed from `PERRY_GC_DIAG=1` in a separate untimed pass) were
  **bit-identical across 3 sessions × 7 repeats** on all 8 probes, so both are
  gated everywhere. RSS (≤0.41% spread) and wall time (≤0.75% on medians) are
  gated only in the `pinned_host` profile and explicitly marked non-gating in
  `shared_ci`, because a GitHub runner is a different machine class — stated in
  `tolerances.json` rather than hidden behind a band too wide to fire.
- **Checker** `benchmarks/gc_ratchet/gc_ratchet.py` with `measure` / `assemble` /
  `check` / `validate`. Integrity problems fail rather than skip: a missing or
  extra probe, a probe whose output stops matching the Node oracle, fewer repeats
  than the baseline, a platform mismatch, or a tampered summary. Evacuation
  counters are gated two-sided, because a collector that copies *fewer* objects
  has changed and must be re-pinned deliberately.
- **CI** `.github/workflows/gc-ratchet.yml`, a standalone job with no
  `continue-on-error` and no pipe swallowing the checker's exit status — the
  shape `gc-stress` has, which let a regression sit through three merges. Not yet
  promoted to a required status check; the promotion command is in the PR body,
  to be run once the job has been green once.
- **Validated end to end against a real collector change**, not just injected
  JSON: under `PERRY_CONSERVATIVE_STACK_SCAN=full` — the mechanism the campaign
  adopts — the gate exits 1 with 60 regression rows and retention up 364%–5371%,
  while the control arm reproduces the baseline exactly and exits 0. That A/B
  also surfaced a design bug, fixed here: the "probe ran no minor collection"
  rule was in `measure`, so a collector that stopped running copying minors
  produced a harness error blaming the probe instead of a regression; it now
  lives in `validate_artifact`.
- `tests/test_gc_ratchet.py` — 32 tests, one per failure mode, including a
  parametric test that walks every gating metric in every profile and asserts
  each independently turns the job red.
