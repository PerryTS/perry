Primitive inline objects reuse the general walker's field preflight, input
root, prototype handling and property order. Once that preflight rules out
callbacks, the emitter borrows the keys and values once instead of retrieving
them repeatedly through the root handle. Output uses the same scalar encoders.
Descriptors, class instances, overflow, nested values, BigInt and raw-pointer
shapes retain the general traversal. No new managed scratch, GC scheduling or
production GC policy changes are included.

138 JSON-filter runtime tests and 33 compiled Node comparisons pass. The new
compiled wide-object witness also matches Node on the preceding release; it
covers property ordering, retained output and mutation, late getters and
non-enumerability, nested toJSON results, default/custom prototypes, a replacer
and spacing. A unit test emits 128 fields 1000 times without managed growth,
collection or added handles. The root-holder inventory retains its prior
unverified frontier. All 24 moving-GC stress runs match Node and show scheduled collections,
copying and live object movement. The full CPU/RSS matrix and paired follow-up are now qualified.

The full matrix contains 152 output verifications, 456 timing trials and 432
memory trials with 33 clean external observations. The 350-trial paired check
covers 25 cases, with 25 clean observations. Both windows pass their load and
competing-process gates. The initial setup attempt omitted the fixture manifest
and stopped before any trial; it is preserved in setup-failed-1.

Paired wide-object stringify improves 26.28%, from 2475.11 to 1824.61 microseconds.
The full matrix reports 1825.25 microseconds versus Node 6367.79 and Bun 664.71;
Perry still needs 2.75 times Bun's CPU for this case. Peak RSS remains about
70.94 MiB. Numeric parsing improves 1.36% and 1 MB object-root parsing 0.89%
against the empty-object release in the paired window. Neither establishes
resolution of their older regressions against earlier checkpoints.

Paired regressions remain: null parse +4.80%, inline-string parse +3.96%,
tiny-object parse +2.21%, small-record parse +0.73%, small-record stringify
+2.15%, wide-object parse +0.98%, and empty-object parse +0.22%. The apparent
escaped-stringify full-matrix slowdown does not reproduce (paired -0.06%,
overlapping ranges). All cases and ranges remain in recheck-primitive-object.

The inventory stays at 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS targets.
This is a development checkpoint, with no-regression and all-row acceptance
still open. The source is 1196a5f84c2976d126bd193839ed00588643234a; all 43 recorded
source hashes were verified against that commit. No managed GC production or
policy changes accompany this candidate.
