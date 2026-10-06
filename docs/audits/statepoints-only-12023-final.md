# #12023 final pre-PR verification

The owner authorized shipment and accepted the previously measured tsc cycle increase as a stack-home/L1 traffic trade, with a fix-forward follow-up. The requested final pre-PR checks pass.

Source identities: main `4b6f9f75f62314d5b0acb0d37735498f0db6fe42` versus measured implementation `8a2c7041a4d74fd41e2c693e82e52b469577466a`. These are the source identities for the recorded correctness and measurement runs, including RegExp S4. The final branch is subsequently rebased onto main `81e65f33ebdc72d75431b03427dcbfb2f153d7b1`; the documentation-only correction below does not change rooting semantics.

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

## Scope and documentation correction

Version metadata matches current main exactly. Workflow switch removals and all statepoint audit files remain. The coordinator corrected the earlier instruction to strip the documentation changes: those files must stop advertising the removed switches.

The two historical benchmark labels in `docs/engine-plan.md` were updated from current main, preserving the measurements without advertising a deleted switch. Catalogs were also restored from current main, then regenerated from the current English sources with `docs/i18n.sh extract` and `sync`, using the repository's pinned mdbook 0.5.4, i18n helpers 0.4.0 and gettext 0.21. Main's catalogs were stale relative to its newer sources, so regeneration includes those source updates rather than reverting them.

Gettext retained obsolete entries and fuzzy translations containing the removed switch names. Gettext's `msggrep` removes only entries containing those names; a subsequent repository sync restores current source entries with English fallback where their translations require an update. Unrelated obsolete entries and translations are retained. No old branch catalog was copied, and the env-knob checker remains unchanged.

| Documentation correction check | Result |
|---|---|
| Env-knob drift gate and self-test | pass |
| Ten catalog format checks | pass |
| Repeat extract/sync freshness | identical catalog hashes |
| English and ten translated documentation builds | pass |
| Documentation links and checker self-test | pass |
| I18n toolchain tests | four pass |
| fmt | pass |
| Release build sanity: compiler and static runtime/stdlib wrappers | `cargo check` passes with eight jobs on CPUs 0–55 |

Only documentation and generated catalogs changed in this correction, so the previously recorded runtime, GC, dominance and witness results were not rerun. Rooting semantics remain unchanged. Scripts and receipts for this correction are in `/root/claude-lanes/sp-work/docs-fix`.

Raw scripts, measurements, diagnostics and the verified binary archive are preserved in `/root/claude-lanes/sp-work/final-pr`. Owned build targets were removed. No push or PR was performed.
