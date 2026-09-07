# Scalar parsing and object-key planning investigation

The fixed-length scalar parser removes the tiny-value CPU regression introduced by structural depth scanning. The compact key-plan experiment is rejected: it makes planning frames smaller but does not improve small-record stringify, and several other stringify rows become slower.

Version stays 0.5.1520. The selected no-new-regression reference remains depth22 (`ae9283ca3`). Scalar31 (`8d8c4cac0`) is an experimental CPU improvement with unresolved RSS increases; key33 (`050860971`) is preserved only as an unsuccessful experiment. Neither changes GC policy.

Scalar31 explicitly constructs the canonical inline-string representation for each validated length from zero through five. Its native noncontainer parser shrinks from 425 to 276 instructions and from a 144-byte to a 96-byte frame. Existing UTF-8, escape, whitespace, number and parse-boundary behavior remains intact.

Key33 replaces 40-byte general scalar plans for keys with 16-byte pointer-free length plans. Native flat/record emitters shrink, but the measured target does not improve: small-record stringify is 0.283949 µs versus 0.282812 µs for scalar31 in the same window (+0.40%). Retained-memory peak medians rise in 34/40 groups versus scalar31, ranging from −64 to +128 KiB. Smaller stack frames alone do not establish a useful optimization.

[Scalar31: all 38 CPU/RSS rows](launch32/results/recheck/all-38.md) and [key33: all 38 CPU/RSS rows](launch34/results/recheck/all-38.md) include both comparison engines. Both candidates have median CPU at or below both Node and Bun in 17/38 rows, and peak RSS at or below both in 32/38 rows. The older full147 target inventory and known semantic gaps remain open.

| Scalar31 compared with depth22, launch32 | Change |
|---|---:|
| null parse CPU | +0.017%, overlapping ranges |
| single-character string parse CPU | −0.028%, overlapping ranges |
| 1 MiB record-object parse CPU | −9.70% |
| escaped 1 MiB parse CPU | −0.91% |
| 20 MiB object roundtrip complete-lifetime CPU | −3.77% |
| main peak-RSS changes | −48 to +752 KiB |
| retained peak-RSS changes, 40 groups | −112 to +64 KiB; 13 positive |
| retained current-RSS changes, 40 groups | −160 to +48 KiB; 11 positive |

No main CPU row in launch32 has separated observed ranges in the slower direction. This is not proof of no regressions: main escaped-parse peak RSS is +752 KiB, and small-record stringify lifetime medians rise by 0.45–1.01% depending on retention mode. Raw ranges are retained.

Key33 has five main rows with separated slower CPU ranges versus depth22: records-array stringify at 16 KiB (+0.375%), 1 MiB (+0.507%) and 8 MiB (+0.556%), 20 MiB record-object stringify (+0.286%), and wide-object stringify (+0.984%). Main peak RSS ranges from −1680 to +144 KiB versus depth22; escaped parse is +96 KiB. Parse source is unchanged from scalar31, so the lower escaped RSS alone does not prove that compact plans resolve conservative retention.

Each window contains 228 output checks, 1140 timed trials, 480 retained-memory trials and 84 complete-lifetime trials. Both qualify with 77 clean monitoring observations, zero competing workers and zero XProtect observations above 5% CPU. Entry/exit one-minute loads are 1.458→1.978 for launch32 and 1.500→2.319 for launch34. Fixed immutable executable paths, a common managed argv[0], CWD and fixtures retain the previously qualified launcher method. Array-root parse can defer materialization; stringify starts with materialized inputs.

Scalar31 passes 3288 runtime tests; key33 passes 3290 (four ignored each). Both pass 40 compiled Node comparisons and 52 seeded moving-GC checks plus default/off controls. Seeded checks require collections, copying minors, moved objects and loop polls to be nonzero. The inherited root-array prototype `toJSON` mismatch is reference-compared, not counted as Node parity. Node-version and root-holder inventories pass. Inherited file-size failures (`inprocess.rs`, 2416 lines; HIR `lower/tests.rs`, 2009) and three address-ratchet findings remain explicit in saved logs.

The next isolated experiment uses scalar31 as its base and combines key ordering checks, output planning and own-key `toJSON` exclusion into one pass. Class and prototype checks, output allocation and GC rooting remain in place. It must earn acceptance through measurements.

Run `python3 benchmarks/json_performance/results/json-key-investigation/verify.py` to check the saved evidence against its source commits. This historical verifier intentionally does not require the current checkout to equal either experimental source.
