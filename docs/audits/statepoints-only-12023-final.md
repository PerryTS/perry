# #12023 final pre-PR verification

The owner authorized shipment and accepted the previously measured tsc cycle increase as a stack-home/L1 traffic trade, with a fix-forward follow-up. The requested final pre-PR checks pass.

Source identities: main `4b6f9f75f62314d5b0acb0d37735498f0db6fe42` versus measured implementation `8a2c7041a4d74fd41e2c693e82e52b469577466a`. The branch is rebased onto this freshly fetched main, including RegExp S4.

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

## Scope and additional lint finding

Version metadata, translation catalogs and `docs/engine-plan.md` match main exactly. Workflow switch removals and all statepoint audit files remain.

Restoring the catalogs and engine plan exposes stale historical switch references to `scripts/check_gc_env_knobs.py`; that additional documentation check fails. The checker was neither weakened nor silenced. This finding was raised with the owner while the requested checks continued; a coordinator decision on the documentation scope remains pending.

Raw scripts, measurements, diagnostics and the verified binary archive are preserved in `/root/claude-lanes/sp-work/final-pr`. Owned build targets were removed. No push or PR was performed.
