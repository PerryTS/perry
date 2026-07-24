# #6812 pre-implementation coverage baseline

Collected on 2026-07-23 before changing any compiler or runtime source.

## Provenance and protocol

- Source: `origin/main` commit
  `20f19758f370f7616530824218d87f46a67cd3f0`, which is the required #6811
  merge.
- Clean worktree branch: `perf/6812-object-write-generalization`.
- Isolated Cargo target:
  `/private/tmp/perry-6812-target.qyHg3D`.
- Compiler: Perry 0.5.1264, built with the issue's exact release command and
  the repository release/LTO profile.
- App compilation used the default auto-optimizing pipeline. Neither
  `PERRY_NO_AUTO_OPTIMIZE` nor a codegen-unit override was set.
- Host: macOS 26.5 (25F71), Apple M1 Max, arm64, 10 logical CPUs.
- Node: v26.3.0.
- Rust: rustc 1.96.1, cargo 1.96.1.
- Each matrix cell had one Node/Perry warmup followed by 15 strict
  Node-then-Perry pairs. The runner compared `(writes, checksum)` after every
  pair and retained all samples without outlier filtering.
- No old Perry, Node, Cargo, rustc, or matrix process was active when the
  definitive capture began. Sporadic OS/UI interruptions remain visible in
  the raw data instead of being discarded.

The exact historical canonical source produced:

| Implementation | Raw `write_ms` samples | Median | Sink |
|---|---|---:|---:|
| Node | `[7, 8, 7, 7, 8, 7, 8, 8, 7, 7, 8, 8, 8, 8, 7]` | 8 ms | 9595200 |
| Perry | `[5, 6, 6, 5, 6, 6, 6, 5, 6, 6, 6, 6, 5, 6, 6]` | 6 ms | 9595200 |

The one-millisecond shift from the published 7/5 medians is `Date.now()`
quantization at this duration; Perry still beats Node on the exact unchanged
source.

## Executable matrix

| Dimension | Cell | Writes | Node median | Perry median |
|---|---|---:|---:|---:|
| Key | `o.x` | 120000000 | 131 ms | 689 ms |
| Key | `o["x"]` | 120000000 | 132 ms | 693 ms |
| Key | stable `o[k]` | 120000000 | 138 ms | 2425 ms |
| Key | alternating dynamic keys | 24000000 | 183 ms | 625 ms |
| RHS | numeric scalar | 120000000 | 133 ms | 698 ms |
| RHS | pointer-capable value | 96000000 | 120 ms | 1480 ms |
| RHS | allocating literal | 19200000 | 119 ms | 11126 ms |
| RHS | function call | 108000000 | 121 ms | 3260 ms |
| Receiver shapes | monomorphic | 120000000 | 135 ms | 706 ms |
| Receiver shapes | 2-shape | 96000000 | 131 ms | 5432 ms |
| Receiver shapes | 4-shape | 60000000 | 108 ms | 3330 ms |
| Receiver shapes | transition before loop | 120000000 | 131 ms | 681 ms |
| Fields/iteration | 1 | 120000000 | 131 ms | 692 ms |
| Fields/iteration | 2 | 240000000 | 148 ms | 138 ms |
| Fields/iteration | 4 | 384000000 | 137 ms | 1444 ms |
| Fields/iteration | 8 | 384000000 | 102 ms | 1272 ms |
| Loop form | single counted loop | 120000000 | 163 ms | 2249 ms |
| Loop form | current nested loop | 240000000 | 149 ms | 138 ms |
| Loop form | stable local bounds | 192000000 | 129 ms | 868 ms |
| Loop form | non-zero inner start | 192000000 | 120 ms | 872 ms |
| Storage | inline existing slot | 120000000 | 132 ms | 693 ms |
| Storage | wide/overflow object | 120000000 | 180 ms | 7154 ms |
| Receiver kind | anonymous object | 120000000 | 109 ms | 692 ms |
| Receiver kind | class instance | 120000000 | 108 ms | 700 ms |
| Receiver kind | class-id-zero plain object | 120000000 | 110 ms | 6356 ms |

Every one of the 375 measured Node/Perry pairs had identical write counts and
checksums.

## Deterministic path classification

The retained 856,485-byte pre-optimization LLVM module was classified by
function-local calls to `js_put_value_set`, `js_put_value_set_ic_miss`, and
`js_object_array_numeric_write2_guard`. The timed loop was also inspected
separately when its function contains setup writes.

| Cell | Timed path | IR calls (`generic`, `PIC`, `clone`) | Reason / rejection |
|---|---|---:|---|
| `o.x` | static PIC | 0, 1, 0 | Literal key and call-free numeric RHS. |
| `o["x"]` | static PIC | 0, 1, 0 | Lowers to the same literal-key form as dot access. |
| stable `o[k]` | generic | 1, 0, 0 | Key is a local expression, not `Expr::String`. |
| alternating keys | generic | 1, 0, 0 | Dynamic PropertyKey conversion/identity stays semantic. |
| numeric RHS | static PIC | 0, 1, 0 | Safepoint-free scalar value. |
| pointer RHS | static PIC | 0, 1, 0 | Safepoint-free, but retains layout/string-alias/barrier handling. |
| allocating RHS | generic | 1, 0, 0 | RHS can allocate after the receiver reference is evaluated. |
| function RHS | generic | 1, 0, 0 | RHS call is a safepoint and may throw/observe user code. |
| monomorphic shape | static PIC | 0, 1, 0 | Stable discriminated ShapeId token primes and hits. |
| 2-shape | static PIC, runtime miss | 0, 1, 0 | One-entry cache thrashes on alternating shape tokens. |
| 4-shape | static PIC, runtime miss | 0, 1, 0 | One-entry cache thrashes on four shape tokens. |
| transition before loop | static PIC | 0, 2, 0 | Setup transition has its own site; timed site sees one stable post-transition token. |
| 1 field | static PIC | 0, 1, 0 | Whole-loop matcher requires exactly two stores. |
| 2 fields | whole-loop clone | 0, 2, 1 | Once-per-nest proof succeeds; both PIC sites remain in the semantic clone. |
| 4 fields | four static PICs | 0, 4, 0 | Whole-loop matcher requires exactly two stores. |
| 8 fields | eight static PICs | 0, 8, 0 | Whole-loop matcher requires exactly two stores. |
| single counted loop | static PIC | 0, 1, 0 | Whole-loop proof requires the dense nested-array form. |
| current nested loop | whole-loop clone | 0, 2, 1 | Exact #6811 shape. |
| stable local bounds | static PICs | 0, 2, 0 | Counted-loop matcher requires literal bounds. |
| nonzero inner start | static PICs | 0, 2, 0 | Dense-prefix proof deliberately starts at zero. |
| inline storage | static PIC | 0, 1, 0 | Existing slot is within the physical inline allocation. |
| overflow storage | static PIC, runtime miss | 1 setup, 1, 0 | Dynamic setup is generic; timed slot 4 cannot prime against a four-slot inline allocation. |
| anonymous object | static PIC | 0, 1, 0 | Compiler-created regular object has a nonzero class/shape identity. |
| class instance | static PIC | 0, 1, 0 | Eligible regular instance with stable class/shape identity. |
| class-id-zero object | static PIC, runtime miss | 0, 1, 0 | Parsed plain object fails the runtime `class_id != 0` eligibility guard. |

The key measured discontinuity is the bounded static numeric nest:

- two fields: 240M stores in 138 ms through the transactional clone;
- one field: 120M stores in 692 ms through a per-store PIC;
- four fields: 384M stores in 1444 ms through four per-store PICs.

The four-field cell is the highest-volume sound miss in the matrix and is
10.5× slower than Node, while the established two-field proof is already
slightly faster than Node.

## Sample evidence

The stripped executable was linked once more with `PERRY_LINK_MAP` (which does
not alter generated code) and sampled with macOS `sample`.

For stable dynamic-key writes, 1,204 of 1,466 main-thread samples (82.1%) were
under `js_put_value_set`. Of those, 519 reached
`try_existing_own_data_overwrite`, and 108 were under
`runtime_store_jsvalue_slot`, with further frames in layout-note and write
barrier work. The addresses were resolved against these linker-map ranges:

```text
0x100489E00 + 0xBBC  js_put_value_set
0x100298750 + 0x2D0  try_existing_own_data_overwrite
0x10014AA44 + 0x0AC  runtime_store_jsvalue_slot
```

For the four-field static cell, 441 of 448 post-startup runnable samples
(98.4%) were directly in the inlined `main` body, with no child runtime frame.
Together with the four PIC call sites retained in IR, this shows the cost is
the repeated inlined per-store guard sequence, not a generic-runtime call.
The two-field executable result demonstrates the benefit of paying a bounded
preflight once and then keeping the complete nest call/GC-free.

## Baseline code size

| Artifact | Size |
|---|---:|
| Matrix LLVM IR | 856,485 bytes / 21,966 lines |
| Matrix app object | 278,968 bytes |
| Matrix executable | 6,302,720 bytes |
| Linked `main` text contribution | 47,232 bytes |
| Canonical executable | 6,170,504 bytes |

<details>
<summary>All raw matrix samples and checksums</summary>

| Cell | Checksum | Node ms | Perry ms |
|---|---:|---|---|
| `key_dot` | `122876400` | `[132, 131, 130, 132, 132, 131, 130, 131, 133, 132, 131, 131, 130, 132, 131]` | `[690, 688, 690, 691, 688, 689, 688, 711, 701, 688, 690, 688, 689, 689, 695]` |
| `key_literal` | `122876400` | `[131, 134, 132, 133, 132, 134, 132, 131, 131, 132, 132, 132, 133, 131, 131]` | `[692, 691, 693, 693, 701, 690, 689, 695, 691, 692, 693, 694, 693, 701, 691]` |
| `key_stable_dynamic` | `122876400` | `[138, 138, 140, 139, 136, 136, 135, 148, 140, 141, 138, 136, 139, 137, 140]` | `[2425, 2448, 2431, 2384, 2395, 2383, 2653, 2549, 2442, 2456, 2423, 2419, 2399, 2408, 2435]` |
| `key_alternating_dynamic` | `31194000` | `[183, 266, 184, 179, 194, 185, 183, 312, 182, 181, 178, 183, 181, 182, 180]` | `[637, 796, 840, 846, 623, 625, 790, 667, 610, 613, 605, 644, 614, 607, 610]` |
| `rhs_numeric` | `122876400` | `[133, 139, 132, 134, 136, 133, 132, 132, 133, 133, 132, 132, 133, 134, 133]` | `[699, 711, 697, 702, 698, 697, 700, 696, 706, 697, 684, 683, 695, 701, 712]` |
| `rhs_pointer` | `40800` | `[121, 120, 119, 120, 120, 120, 119, 120, 119, 121, 121, 122, 122, 119, 120]` | `[1487, 1488, 1480, 1480, 1477, 1481, 1478, 1482, 1480, 1478, 1515, 1482, 1481, 1479, 1479]` |
| `rhs_allocating` | `22076400` | `[119, 114, 153, 143, 134, 120, 117, 113, 117, 113, 112, 113, 142, 195, 119]` | `[10936, 12509, 16052, 13348, 11307, 11055, 10870, 10729, 10714, 10823, 10733, 11126, 15021, 13009, 11158]` |
| `rhs_call` | `110876400` | `[123, 191, 124, 119, 126, 119, 118, 121, 129, 120, 129, 119, 145, 120, 121]` | `[3309, 3418, 3311, 3214, 3217, 3260, 3256, 3264, 3300, 3243, 3250, 3589, 3864, 3235, 3218]` |
| `shape_monomorphic` | `122876400` | `[135, 135, 135, 133, 136, 134, 135, 137, 133, 136, 133, 135, 135, 135, 135]` | `[709, 708, 705, 706, 721, 707, 713, 700, 697, 691, 705, 709, 695, 712, 689]` |
| `shape_two` | `98876400` | `[130, 131, 150, 131, 130, 134, 137, 131, 130, 132, 134, 132, 130, 132, 130]` | `[5330, 5963, 5401, 5432, 7717, 5527, 5456, 5429, 5440, 5433, 5432, 5368, 5320, 5352, 5326]` |
| `shape_four` | `62876400` | `[107, 106, 108, 109, 107, 108, 109, 106, 109, 110, 114, 109, 106, 108, 107]` | `[3330, 3332, 3323, 3327, 3395, 3337, 3327, 3341, 3392, 3373, 3387, 3317, 3284, 3296, 3288]` |
| `shape_transition_before_loop` | `122876400` | `[130, 131, 131, 131, 131, 131, 133, 131, 130, 133, 131, 132, 132, 132, 132]` | `[679, 680, 691, 680, 682, 680, 681, 682, 680, 681, 682, 682, 681, 681, 682]` |
| `fields_one` | `122876400` | `[131, 132, 130, 131, 132, 131, 131, 131, 131, 132, 132, 131, 130, 131, 131]` | `[691, 695, 693, 692, 690, 689, 692, 689, 689, 693, 713, 691, 692, 693, 690]` |
| `fields_two` | `239995200` | `[149, 148, 150, 148, 147, 148, 149, 150, 148, 148, 148, 148, 147, 149, 149]` | `[138, 138, 139, 139, 137, 138, 137, 138, 137, 138, 138, 138, 138, 137, 140]` |
| `fields_four` | `383990400` | `[136, 136, 137, 136, 137, 137, 136, 137, 137, 137, 137, 137, 137, 141, 138]` | `[1432, 1446, 1447, 1444, 1441, 1443, 1446, 1445, 1444, 1446, 1443, 1442, 1446, 1455, 1441]` |
| `fields_eight` | `383980800` | `[102, 102, 101, 100, 101, 102, 104, 102, 102, 112, 101, 103, 102, 114, 102]` | `[1260, 1262, 1259, 1260, 1273, 1272, 1270, 1661, 1782, 1276, 1591, 1276, 1272, 1266, 1264]` |
| `loop_single_counted` | `287997118800` | `[164, 163, 164, 163, 163, 163, 163, 164, 164, 164, 164, 162, 163, 162, 163]` | `[2286, 2258, 2247, 2250, 2251, 2254, 2248, 2246, 2248, 2247, 2247, 2249, 2249, 2248, 2253]` |
| `loop_current_nested` | `239995200` | `[150, 149, 148, 150, 149, 150, 148, 151, 148, 148, 148, 151, 149, 150, 149]` | `[138, 139, 138, 138, 138, 139, 138, 138, 138, 137, 140, 138, 137, 138, 138]` |
| `loop_stable_local_bounds` | `191995200` | `[130, 129, 129, 129, 129, 128, 129, 130, 130, 130, 132, 130, 130, 129, 128]` | `[882, 865, 868, 873, 869, 916, 864, 868, 867, 868, 869, 882, 887, 866, 864]` |
| `loop_nonzero_start` | `191995200` | `[119, 122, 119, 120, 119, 121, 120, 120, 119, 120, 121, 120, 120, 121, 119]` | `[872, 876, 872, 874, 887, 871, 872, 871, 871, 871, 872, 887, 873, 872, 886]` |
| `storage_inline` | `122876400` | `[132, 131, 131, 132, 132, 133, 132, 142, 136, 133, 132, 134, 132, 131, 132]` | `[692, 693, 693, 694, 692, 693, 695, 694, 693, 686, 684, 682, 695, 692, 692]` |
| `storage_overflow` | `122876400` | `[180, 180, 178, 179, 178, 180, 190, 179, 180, 180, 179, 179, 181, 191, 179]` | `[7169, 7120, 7171, 7149, 7167, 7142, 7154, 7121, 7130, 7139, 7165, 7135, 7174, 7432, 7163]` |
| `receiver_anonymous` | `122876400` | `[110, 110, 108, 109, 109, 110, 108, 109, 109, 108, 110, 108, 109, 109, 110]` | `[692, 693, 693, 692, 692, 692, 693, 693, 692, 693, 691, 694, 692, 692, 698]` |
| `receiver_class_instance` | `122876400` | `[105, 108, 109, 107, 107, 108, 109, 144, 106, 114, 110, 108, 107, 113, 108]` | `[697, 695, 707, 705, 695, 687, 700, 950, 897, 896, 700, 700, 956, 691, 706]` |
| `receiver_class_id_zero` | `122876400` | `[110, 111, 106, 105, 106, 107, 120, 111, 107, 107, 1125, 134, 119, 106, 137]` | `[6381, 6672, 6203, 6112, 6106, 6596, 6356, 6217, 6198, 42809, 20211, 7303, 6197, 6235, 7880]` |

</details>
