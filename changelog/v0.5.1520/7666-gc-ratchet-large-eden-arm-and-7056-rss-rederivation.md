**GC ratchet: the large-Eden copying-minor cadence is pinned and gated (#7481), and #7056's RSS numbers are re-derived under the statepoint default.**

Two halves of one job — they share the pinned quiet host and the same harness.

### The arm (#7481)

Twelve probes all ran the shipped 16 MB nursery cap, so every copying minor this
matrix had ever exercised was small and frequent. #7481 named the gap itself —
"a live copying-minor correctness signal at exactly the cadence the ratchet
probes never exercise" — and its own reproducer is deterministic at
`PERRY_GC_SCAVENGE_NURSERY_MB=64` and absent at 1, 4, 16 and 32 MB.

A probe may now declare the collector it is a probe *of*, in its own source:

```ts
// gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=64
```

`13_large_eden_survivors.ts` carries exactly that. The declaration reaches every
run of that probe — warmup, the timed repeats, both traced runs, and both of
`classify`'s scan modes — and deliberately not `compile_probe`, because these are
`OnceLock` runtime knobs and Perry's object cache keys on every codegen env var
(#6394). Only `PERRY_*`; never `PERRY_CONSERVATIVE_STACK_SCAN` or
`PERRY_GC_DIAG`, which the harness owns; never a repeated key.

**The arm is pinned in the artifact and compared like a metric**, before any band
arithmetic — and that is not ceremony. With the directive deleted the probe's
retention, RSS and wall all read as *improvements* (−80.27%, −37.33%, −27.07%),
so the deletion looks like a win from every one-sided band. Only the arm check
and the two-sided evacuation counters say what actually happened.

**A large Eden on a small retained set runs zero copying minors.**
`arena_growth_full_escalation_due` escalates a minor to a full mark-sweep once
arena in-use clears 32 MB *and* exceeds twice the post-full baseline; a 64 MB
Eden over a ~1 MB live set satisfies both every time. The first draft of this
probe did exactly that — 0 copying minors, 4 full sweeps — and would have been
pinned on a collector it never reached. So `PERRY_GC_SCAVENGE_NURSERY_MB` is not
on its own a "larger Eden" knob: above ~32 MB it is a "no copying minor" knob
unless the workload also holds a live set. Measured on the pinned host:

| `KEEP` | copying minors at 64 MB | at the shipped default |
|---|---:|---:|
| 8,192 | **0** | 15 |
| 131,072 | 1 | 14 |
| 262,144 (shipped) | **4** | 12 |

At the shipped size the four minors free 37, 36, 68 and 68 MB — 49.7 MB per
minor — and the first copies **532,482 objects (32 MB)** in one cycle. The rest
of the suite runs 14.6–16.6 MB per minor on eleven of twelve, and 21.8 MB on
`12_large_live_set`, whose tenured-proportional cap term already raises its Eden
without any knob.

**Shown able to fail**, each arm measured with the real harness and scored by
`check` against the freshly pinned artifact:

| perturbation | verdict | why |
|---|---|---|
| control, nothing changed | `OK` | the pin reproduces on an independent session; probe 13's wall median 3,137 vs 3,121 ms, retention and counters bit-identical |
| `gc-ratchet-env` directive deleted | **FAILED** | arm mismatch, plus `minor_cycles` 4 → 12, `copied_objects` 532,482 → 286,246 |
| `PERRY_GC_SCAVENGE=0` | **FAILED** | the liveness rule fires: `minor_cycles` 4 → 0, `copied + promoted` 1,074,380 → 0, retention +3135% |
| `PERRY_GEN_GC_EVACUATE=0` | `OK` | **the perturbation was inert, not the gate** — see below |

That last row is kept because it is the lesson, not a footnote.
`PERRY_GEN_GC_EVACUATE=0` does **not** gate the copying minor: 4 copying minors
with it set, 4 without. It gates the C4b old-gen policy evacuation and
`gc_force_evacuate_enabled`. A green gate under a knob whose name promises
otherwise reads as "the gate cannot fail"; the correct reading was "nothing was
perturbed", and the discriminator was counting copying minors before trusting
the verdict.

`wt-scavtenure` is **subsumed**: #7432 is merged, its worktree exists on neither
host, and the quiet-host driver's `--check` at this commit against the #7657
artifact reported every one of its 144 cells `ok`, failing on exactly one line —
the new probe's absence. There was no pending re-pin.

### The re-derivation (#7056)

#7056's numbers were taken under the shadow-stack root lowering, which stopped
being the default in #7370. 2x2 over root lowering x nursery cap, 12 probes,
7 repeats, **3 interleaved rotations**, all 72 probe-runs byte-identical to the
pinned Node oracle:

| | statepoint (default) | shadow stack (`PERRY_RS4GC=0`) | ratio |
|---|--:|--:|--:|
| peak RSS, 12 probes | 544,210,944 B | 545,144,832 B | **1.002** |
| retained heap after `gc()` | 114,594,904 B | 114,596,520 B | **1.000** |
| wall | 4,808 ms | 4,894 ms | 1.018 |

**The root lowering is not an RSS lever.** 104 of 108 deterministic cells are
bit-identical; all four that move are on `10_store_receiver_across_alloc` and are
≤0.31% (the shadow stack keeps 7 more objects live at that collection point).
Between-rotation spread: retention 0.000%, peak RSS max 0.621%, wall max 2.779%.
The arms were shown to differ per probe before they were compared —
`statepoint-example` + `addrspace(1)` roots on 12/12 under the default, 0 of both
under `PERRY_RS4GC=0`.

What *did* invalidate parts of #7056 is everything else that shipped since:

- §9's recommendation shipped (#7377); the poll has been default-off since #7161,
  so none of its five arms names anything that ships.
- The cap is still the whole footprint lever, now on 12 probes rather than 8: at
  `PERRY_GC_SCAVENGE_NURSERY_MB=128`, peak RSS **1.911x**, retained heap
  **2.475x**, wall **0.947x** — up to 5.005x and 6.350x per probe. Identical to
  within 0.6% under both lowerings, so it is a pacing result, not a rooting one.
- §7's 4.6x–54.8x false-retention table is **gone, not shrunk**: `classify`
  reports excess 0.00% and spread 0 on all twelve, with no `manual_collect` scan
  site — #7657 removed the cause.
- §6's finding survives: a conservative scan does not degrade the copying minor,
  it **disables** it. `PERRY_CONSERVATIVE_STACK_SCAN=full` under statepoints
  takes `minor_cycles` to 0 on all twelve probes, for 1.98x retention and 1.46x
  peak RSS.
- #7481 no longer reproduces (5/5 runs exit 0 with identical checksums), and does
  run 6 copying minors at cap 64, so the arm is not vacuous.

Not re-derived and stated rather than hidden: #7056's largest RSS numbers
(§3–§5) came from three server-shaped workloads it wrote and never landed.

### One trap worth the four lines

An ad-hoc `measure --probes-dir <copy>` reported
`09_try_catch_roots.heap_used_bytes` at **−17.98%** against the pin, which reads
exactly like drift. It is not: a probe compiled with a `package.json` in scope
retains one more 1 MiB arena block at `gc()` than the same source compiled
without one, and 09 sits on that boundary. Reproduced three ways — repo dir
5,825,256; three separate `/tmp` directories 4,777,624; `/tmp` **plus a copied
`package.json`** 5,825,256 — each stable across repeats, and unmoved by Perry's
known build non-determinism (two builds from one directory differ byte-wise and
report the identical number). The driver and CI always compile from the repo, so
the gate is self-consistent; a copied probes directory outside the repo is a
different compilation, and `benchmarks/gc_ratchet/README.md` now says so.
