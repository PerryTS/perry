# #10863 — measured, against the prediction committed in `10863-prediction.md`

Base `f88acdaa1` (v0.5.1631), host perrymaster, both arms linking the PREBUILT
runtime (verified per build log). Fix arm verified by content: the runtime
archive carries the new `way_encoded_slot` string; the base arm does not.

## Counts — `PERRY_IC_DIAG`, 20M reads

| fixture | counter | before | predicted | measured |
|---|---|---|---|---|
| m64 | megamorphic | 0 (0.0 %) | > 90 %, est. 97.7 % | **96.97 %** ✅ |
| m64 | armed | 18025214 (100.0 %) | < 2 %, est. 0.76 % | **0.758 %** ✅ |
| m64 | fresh | 2 | < 5 %, est. 1.55 % | **2.27 %** ✅ |
| m64 | in_ways | 0 | 0 | **0** ✅ |
| m64 | way_encoded_slot | — | 0 | **0** ✅ |
| m64 | own_inline_primed | 1 | ~1.5 % of primes | **1.56 %** ✅ |
| m64arm | armed | 100.0 % | < 0.5 % | **0.391 %** ✅ |
| m64arm | megamorphic | 0 | > 99 % | **50.0 %** ❌ (see below) |
| m64noarm | fresh/armed/mega | 100/0/0 | unchanged | **100/0/0** ✅ |
| m64inline | fresh/armed/mega | 0.048/0.919/99.03 % | unchanged | **0.048/0.919/99.03 %** ✅ |

The one count I got wrong: m64arm's `megamorphic`. I predicted > 99 % and it is
50.0 %, with 49.6 % `fresh`. After the latch countdown expires the site sits at
state 0 waiting for its 1-in-4096 armer, and state 0 is `fresh`, not
`megamorphic`. Both mean the emitted gate skips the compares; I had conflated
"not armed" with "latched". `armed`, the state that actually costs, was right.

## Cost — marginal instr/read, min of 3, fitted N=500k→5M, 2M flatness point

| fixture | base | predicted | measured | Δ |
|---|---|---|---|---|
| m64 | 757.98 | 748–756 | **737.69** | **−20.29 (−2.68 %)** |
| m64arm | 772.92 | 759.5–761.5 | **755.09** | **−17.83 (−2.31 %)** |
| m64noarm | 759.97 | 759.97 ± 1 | 764.81 | +4.84 (+0.64 %) ❌ |
| m64inline | 1352.85 | ± 4 | **1352.83** | −0.02 (0.00 %) ✅ |
| own3 | 123.00 | no regression | **123.00** | 0 ✅ |
| w4 | 192.00 | no regression | **192.00** | 0 ✅ |
| inh | 349.00 | no regression | **349.00** | 0 ✅ |
| inh3 | 288.00 | no regression | **288.00** | 0 ✅ |

The win is LARGER than predicted on both overflow fixtures. The prediction
capped it at the 12.95 instr/read I had estimated for the emitted `pic.ways`
block by subtracting m64noarm from m64arm. **That estimator was wrong**: the two
programs build different numbers of shapes, so their shape-id tables differ, and
the subtraction carries that difference. The within-fixture A/B — same program,
same emitted code, only the runtime archive differs — is the measurement. ~20
instructions is what the block actually contains (8 dependent loads, 4 compares,
a 4-lane select tree, two branches).

## The miss: m64noarm, +4.84

Predicted 0, measured +4.84 (+0.64 %). Its `PERRY_IC_DIAG` counters are
identical to base, so this is not behaviour. Attributed with a **sabotage arm**
(the same patch with the new arm compiled out via `if false && …`): that arm
measures **764.14, +4.17 over base**, behaviourally identical to base. So the
residual is codegen layout in `pic_prime_get` / `get_field_ic_miss_impl`.

Six structural variants were built and measured on this fixture:

| variant | m64noarm | m64inline | m64 |
|---|---|---|---|
| base | 759.97 | 1352.85 | 757.98 |
| minimal (two-line fix, nothing else) | 767.14 | 1353.87 | 738.75 |
| inline diag + inline arm | 766.97 | 1353.85 | 738.57 |
| cold diag + inline arm | 765.82 | 1352.84 | 737.57 |
| cold diag + outlined arm | 764.98 | 1352.83 | 737.69 |
| **+ split way pass (shipped)** | **764.81** | **1352.83** | **737.69** |
| sabotage (shipped, arm compiled out) | 764.14 | 1355.80 | 761.85 |

Every variant that touches this function lands +4.2..+7.2 on m64noarm. The
shipped one is the best of them and the only one that leaves m64inline exactly
unchanged. m64noarm is the case where NO shape in the rotation carries the key
inline — the site never arms, and today a single inline sighting during warm-up
is enough to arm it forever, which is the case being fixed.

## Sabotage

`if false && state > 0 && evicted`, rebuilt, re-run:

```
an_overflow_rotation_latches_instead_of_staying_armed_forever ... FAILED
  an overflow rotation must spend its life latched: fresh=2 armed=25199 megamorphic=0
a_latched_overflow_site_still_counts_down_and_re_arms ... FAILED
test result: FAILED. 10 passed; 2 failed
```

`fresh=2 armed=N megamorphic=0` is the issue's exact signature. The ten that
still pass include the two guarding "must NOT latch" and the eight pre-existing
#7753 latch-policy tests. Restored; the shipped tree has no `SABOTAGE` marker.
