# #12023 final pre-PR verification

The owner authorized shipment and accepted the previously measured tsc cycle increase as a stack-home/L1 traffic trade, with a fix-forward follow-up. The requested final pre-PR checks pass.

Source identities: main `4b6f9f75f62314d5b0acb0d37735498f0db6fe42` versus measured implementation `8a2c7041a4d74fd41e2c693e82e52b469577466a`. These are the source identities for the recorded correctness and measurement runs, including RegExp S4. The final branch is subsequently rebased onto main `c1c251496bb57e9d90076ab7167a2bfe361e4389`; the documentation-only correction below does not change rooting semantics.

## Growing catch snapshots

The S4 vector starts empty and grows before publishing the new handler depth. Its length stays at least the active depth; pops retain initialized but inactive entries. Jump buffers remain in their fixed box. Scan entry obtains the current storage through `ExceptionState`, and the visitor scans exactly `[..try_depth]`. Restores likewise index through the state at the active handler depth. No vector pointer is cached across growth.

Native frame savepoints retain only expression-temp depth; captured GC values are scanned through the exception snapshot visitor. No production rooting semantics changed during the reconciliation. The 1,024-handler test now additionally checks the length invariant on every push and zero roots after every handler is popped.

## Requested verification

| Check | Result |
|---|---|
| Fresh release SDK, main and head | pass |
| Serial runtime suite | 5,156 passed, zero failed, five ignored |
| Exception/savepoint restore tests and S4 1,024-handler test | pass |
| Evacuation subset, ten fixtures ×two modes ×two arms | 40 runs pass; no output differences |
| Head verification copying minors | 216 |
| Fresh targeted native dominance | 329 functions /11 modules; 2,572 safepoints; zero unrooted/stale |
| Dominance sabotage | 40 planted, 40 caught, zero missed |
| q25/q50/q100/q200 bytes | 18,122 /36,362 /72,862 /138,767; linear gate passes |
| fmt | pass |

The production dominance corpus and its floors remain unchanged. This efficient final sabotage pass uses the ten savepoint/native-root fixtures; the earlier full-corpus receipts remain historical evidence.

## Locked instruction spot checks

Five interleaved pairs per workload per THP mode, core 59, ASLR disabled, inside the measurement lock. Both arms produce matching stdout.

| Workload | Instructions Δ, THP off | Instructions Δ, THP on |
|---|---:|---:|
| tsc | -0.072% | -0.072% |
| commander | -0.266% | -0.271% |
| qs | -0.328% | -0.326% |

Tsc has three copying minors and zero full collections on both arms in both modes. Cycles and L1 misses are retained in the raw spot-check data; this short run does not replace the previously accepted cycle comparison.

## Scope and minimal documentation correction

Version metadata matches current main exactly. Workflow switch removals and the Markdown statepoint audit summaries remain. All six raw JSON audit receipts are removed from the branch; raw measurements remain in the lane's results directories.

The two historical benchmark labels in `docs/engine-plan.md` were updated from current main, preserving the measurements without advertising a deleted switch. Catalogs were restored from current main again after the coordinator rejected the full regeneration diff. The final catalog patch changes only seven entries in the POT and eight entries per translation: paragraphs/examples affected by the deleted switches, the deleted control row, and one obsolete entry. Changed translations use English fallback until retranslated. Every unrelated entry remains byte-identical to main.

The final catalog diff is 255 additions and 532 deletions across eleven files. Each translation changes 65–78 lines; the POT changes 46. There is no blanket reformatting or catalog regeneration in the final tree.

### Baseline catalog freshness

The literal freshness gate is `git diff --exit-code -- docs/po` after `docs/i18n.sh extract` and `sync`, in the release-tag/manual `Deploy Docs` workflow. PR CI instead runs `docs/scripts/test_i18n_toolchain.py`. The pinned toolchain is mdbook 0.5.4, i18n helpers 0.4.0 and gettext 0.21.

Running that same pinned extraction/sync against a separate current-main checkout proves the full rewrite predates this patch: main changes eleven files by **+417,770 /−443,927 lines** and fails freshness. A separate snapshot of the minimally edited head changes eleven files by **+417,593 /−443,672 lines** and fails the same check. Both snapshots are kept separate from the branch catalogs. The comparison checkout was main `81e65f33e`; the later rebase onto `c1c251496` leaves all catalog sources, tooling and workflow inputs byte-identical. The knob, documentation and toolchain checks and fmt also pass after that rebase. This is a reported inherited deployment-gate failure, not a passing freshness check or a weakened gate.

| Documentation correction check | Main | Head |
|---|---|---|
| Env-knob drift gate and self-test | pass | pass |
| Ten catalog format checks | pass | pass |
| Release docs extraction/sync freshness | inherited failure | same failure |
| English and ten translated documentation builds | pass | pass |
| Documentation links and checker self-test | pass | pass |
| I18n toolchain tests | four pass | four pass |
| fmt | pass | pass |

No production code changed in either documentation correction. The preceding correction's release compiler/static-wrapper `cargo check` passed; runtime, GC, dominance and witness results were not rerun unnecessarily. Rooting semantics remain unchanged. Comparison scripts and receipts are in `/root/claude-lanes/sp-work/docs-minimal`.

Raw scripts, measurements, diagnostics and the verified binary archive are preserved in `/root/claude-lanes/sp-work/final-pr`. Owned build targets were removed. No push or PR was performed.
