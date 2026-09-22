# Acceptance corpus: mechanism coverage

Audit date: 2026-09-22. Baseline: main `a022cf2e4`, Linux x86-64,
stock GC policy. Fixture size and arguments are part of every coverage claim.
`light` describes build weight, not whether a fixture reaches GC pressure.

`corpus.py` measures shape mints; a shape-ratio PASS is **not** a GC-pacing
verdict. Use the separate `gc_reclaim_check.py` witness below before treating
a flat timing comparison as evidence about old-reclaim pacing. Diagnostics
are for mechanism coverage; collect performance numbers separately with them off.

| Corpus row (arguments) | Mechanism the source is designed to exercise | Old-reclaim pacing evidence at these settings |
|---|---|---|
| `tsc` (`1`) | Real compiler AST/object layouts, property transitions, parsing | Not established for pinned one-iteration row in this audit. Do not transfer evidence from three-iteration runs. |
| `zod` (`1`, `zodwork.ts`) | Real schema construction/validation, repeated layouts | Campaign diagnostic: zero fulls and trigger observations; unsuitable for old-pacing acceptance. |
| `opencode` (`models`) | Real application startup, module graph and model enumeration; release defines required | Not measured here. No old-pacing claim. Heavy build; TUI invocation does not terminate and is not this row. |
| `mx_acc` (no argument) | Accessor descriptors on a two-field object | **Unexercised:** `N = Number(process.argv[2])`; omitted argument means loop never runs. No GC evidence. |
| `mx_acc1` (no argument) | Accessor descriptors on a one-field object | **Unexercised:** same missing iteration argument. |
| `mx_accd` (no argument) | Data descriptors on a two-field object | **Unexercised:** same missing iteration argument. |
| `mx_accd1` (no argument) | Data descriptors on a one-field object | **Unexercised:** same missing iteration argument. |
| `mx_del` (no argument) | Delete/re-add shape transitions | **Unexercised:** same missing iteration argument. |
| `mx_diff` (no argument) | Fresh dynamic property names per object; layout-diversity adversary | **Unexercised:** same missing iteration argument. |
| `mx_pool` (no argument) | Reused dynamic property-name pool | **Unexercised:** same missing iteration argument. |
| `mx_same` (no argument) | Repeated fixed-name transition chain | **Unexercised:** same missing iteration argument. |
| `c_manylayouts` | 400 distinct object literal layouts; layout-count floor | Campaign diagnostic: zero fulls and trigger observations. |
| `c_samecontent` | Same ordered key set at multiple allocation sites; content-key sharing | Old-GC coverage not measured. Shape activity alone does not establish it. |
| `c_widegrow` | 120-property growth chain; transition reuse | Campaign diagnostic: zero fulls and trigger observations. |
| `c_protograph` | Inheritance/prototype and own-property layouts | Campaign diagnostic: zero fulls and trigger observations. |
| `gc_reclaim` (`64 6`) | 64 MiB bounded live strings, six replacement rounds, short-lived nursery objects | **Measured:** 12 fulls, 17 triggers, 9 confirmed productive non-retaining old reclaims. See measured result below. |
| `retain.ts` (`900000 60`, companion in `/root/l14gc`, not a corpus row) | Large persistent live object graph plus churn; retaining-regime adversary | Campaign control: approximately 179 MB old-gen, retaining regime. It cannot validate the non-retaining pacing arm; fulls alone do not make it eligible. |

The four campaign controls reported process peaks of 41–49 MB and zero fulls
and trigger observations (recorded in `/root/l14gc/retain.ts`). Those are prior
campaign observations, not freshly measured results from this audit. RSS is
not old-gen occupancy: use the diagnostics, not an RSS/threshold comparison,
to establish coverage. Other mechanisms listed above are source-supported
intent; do not read them as measured dynamic coverage.

The eight `mx_*` rows need an explicitly pinned positive iteration count and
fresh censuses before their shape ratios can be used. This audit records that
deferral and does not silently change the population behind historical results.

## Running the old-reclaim witness

Build a paired compiler/runtime/stdlib in your own worktree with the campaign
build-slot wrapper. Pin `PERRY_RUNTIME_DIR`, disable cache/auto-optimization,
and preserve the build status; do not substitute another lane's WIP compiler.

```sh
export PATH=/root/.cargo/bin:$PATH
export LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_BUILD_JOBS=4
/root/perry-build-slot.sh cargo build --release \
  -p perry -p perry-runtime-static -p perry-stdlib-static
PERRY_RUNTIME_DIR="$PWD/target/release" PERRY_NO_CACHE=1 PERRY_NO_AUTO_OPTIMIZE=1 \
  /root/perry-build-slot.sh target/release/perry build \
  test-files/gc-pacing-corpus/reclaim.ts -o /tmp/gc-reclaim
python3 scripts/gc_reclaim_check.py /tmp/gc-reclaim --out-dir /tmp/gc-reclaim-evidence
```

The fixture uses large pointer-free strings, which the arena's general
large-object route allocates in old-gen. Its 64-slot ring holds at most 64 MiB
of string payload after each assignment, continuously replacing older data.
Each replacement also allocates 2,048 nursery objects, retaining only the last.
This matters: the first candidate omitted nursery churn and did run fulls,
but its productive fulls had `retaining=true` after empty-young minors. The
checker rejected it. Adding young deaths changes that measured signal and
produces productive `retaining=false` old-reclaim decisions.
This exercises old-reclaim pressure without constructing 900,000 live objects,
forcing `gc()`, or lowering any threshold. It does not claim to cover object
graph tracing, remembered sets, evacuation, or promotion-triggered reclamation.
The checksum reads every final string and checks sizes/characters against an
independently computed oracle. `--check-log` validates diagnostic coverage only;
the normal binary invocation additionally checks process success and checksum.

The checker deliberately distinguishes `kind=OldReclaim` from
`kind=PromotedCohort`: both can lead to `trigger=OldGenBytes`, but only the
former establishes that this pacing decision ran. A nonzero full count alone
does not distinguish them. Productivity uses the associated sweep's freed-byte
counter. In addition, it requires the first subsequent decision's
`old_baseline` to be at least 1 MiB below the pre-full `old_reclaimable` value.
`finish_full_old_reclaim_baseline()` records surviving old pressure; subsequent
promotion credits can only raise it. This lower bound proves old reclamation,
whereas the sweep counter alone could report only nursery garbage. Neither
counter is a claim about bytes returned to the OS or RSS reduction.

Sabotage the mechanism by running the same binary with `--slots 1 --rounds 1`.
It must finish with the correct checksum and then fail with `GC COVERAGE FAIL`.
Also strip all `[gc-full]` lines from a saved positive diagnostic and require
`--check-log` to fail. Restore the normal fixture and require a positive run.
Parser tests run sequentially:

```sh
/root/perry-suite.sh python3 scripts/test_gc_reclaim_check.py
```

## Measured witness and sabotage

Clean main compiler/runtime build, release with 16 codegen units; normal native
root lowering, default GC, single-threaded fixture. No runtime code was changed.

* Stock `64 6`: **12** full collections, **17** trigger observations, **11**
  above-threshold non-retaining OldReclaim fulls, **9** confirmed old baseline
  drops totaling at least **293,610,240 bytes**. Instrumented wall time **0.166 s**;
  this is a fixture-cost observation, not a performance comparison.
* Checksum: `reclaim 64 6 470582468`, identical under pinned Node 26.5.1.
* Small `1 1` control: correct checksum, **0** fulls and **0** triggers;
  checker exits **1**, `GC COVERAGE FAIL`.
* Removing every `[gc-full]` record also exits **1**, `GC COVERAGE FAIL`.
  Disabling the retaining guard in an isolated checker copy makes its own
  `test_retaining_is_not_eligible` fail; restoring it returns the suite green.
  No mutated checker remains installed.
* Sequential parser suite: **11 passed**. It rejects retaining, manual,
  promoted-cohort, nursery-only reclaim, missing/changed diagnostics, and
  below-stock-threshold cases.
* All eight omitted-argument `mx_*` rows print `0` under pinned Node 26.5.1,
  corroborating the source-level missing-iteration finding.

Representative diagnostic sequence (irrelevant fields omitted):

```text
[gc-trigger] site=alloc_point kind=OldReclaim old_reclaimable=83889336 old_baseline=50333880 old_threshold=50331648 retaining=false
[gc-full] site=alloc_point_old_reclaim trigger=OldGenBytes count_at_site=3 old_reclaimable=83889336 old_baseline=50333880
[gc] blocks: ... freed_bytes=17006992 ... block_skip_reclaimed_bytes=16777784
[gc-trigger] site=safepoint kind=OldReclaim old_reclaimable=101715672 old_baseline=67111608 old_threshold=50331648 retaining=false
```

The baseline drop in that sequence proves at least 16,777,728 old bytes were
reclaimed. Raw evidence is preserved on the campaign host under
`/root/wt-astra-gates/gate-evidence/verified/{gc-diag.txt,stdout.txt,coverage.json}`.
Compiler SHA-256: `d553e0a3f378a71b80ad7c353feb000a344df7a79f4eb3033af1292a117757ac`.
Runtime archive SHA-256: `19cc952e11efed942b0719aefdabe5849ab234e690d91378dc92ca68d6d913a9`.
Fixture binary SHA-256: `d06c8016be979e46e5a7ace811e2b7e70e5b5f3f0ff466edb1ef363bb51ed9da`.

The campaign `/root/matrix/corpus.tsv` registers `gc_reclaim` as `light`, with
arguments `64 6`; `CORPUS-MECHANISMS.md` and `gc_reclaim_check.py` sit alongside
`corpus.py`. Shape census runs still require their existing instrumented build;
this audit does not claim a shape-census result for the new row.

## GC gate health

Both GC-root-dominance jobs already fail on main in scheduled run
[35689113373](https://github.com/PerryTS/perry/actions/runs/35689113373), at the
pre-build symbol-existence audit. This was reproduced unchanged at `a022cf2e4`:
147 poll-capable entries, 3,876 scanned exports, exit 2. It is not a measured
root-dominance violation. The two regex callback exports are macro-generated
and invisible to the literal-source scanner; the rate-limit export really was
deleted by `dcb8a760a`. A freshly built main runtime archive also exports both
regex names (`nm -g --defined-only`). Repair and full-job verification are
**deferred**, with mechanism, exact jobs, and acceptance criteria filed in
[issue #10389](https://github.com/PerryTS/perry/issues/10389#issuecomment-5774661628).
No audit entry or gate budget was relaxed.
