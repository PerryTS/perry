# #6812 bounded object-write generalization results

Collected on 2026-07-23/24 from the same clean worktree, host, isolated Cargo
target, release/LTO compiler, and default auto-optimized runtime described in
[`baseline.md`](baseline.md). All post-change application objects were
regenerated with `PERRY_NO_CACHE=1`; application execution used the normal
production configuration.

## Implemented slice

The #6811 whole-nest proof is generalized from exactly two writes to a bounded
descriptor of one through four static fields:

- the compiler preserves property/source order (including duplicates), proves
  every arithmetic intermediate finite over the complete loop domain, performs
  one preflight, and emits a finite call/GC/side-exit-free fast clone;
- the runtime validates the dense receiver prefix, shared shape, object state,
  physical slot bounds, and intact typed descriptors before publishing four
  packed 16-bit slot lanes;
- finite numeric bits are admitted in verified raw-f64 or ordinary JSValue
  typed slots;
- the once-only lookup compares live string-pool keys by content and does not
  accidentally depend on whether another site interned the same name;
- the previous two-field exported ABI remains available for cached generated
  objects;
- a failed proof enters the original semantic clone before any store. Dynamic
  keys, safepointing/pointer RHS expressions, mixed shapes, holes, short
  arrays, descriptors/mutability flags, overflow slots, class-id-zero objects,
  nonzero/variable bounds, and five or more writes remain bounded fallbacks.

Set `PERRY_TRACE_OBJECT_ARRAY_WRITE_GUARD=1` to print a cold rejection reason.
The cold logger is reached only after a proof branch has already failed; a
successful production guard performs no environment lookup or logging.

## Required 15-pair measurements

One warmup was followed by 15 strict Node-then-Perry pairs. Every pair matched
the write count and checksum.

| Fields/iteration | Baseline Perry | Final Node | Final Perry | Change |
|---:|---:|---:|---:|---:|
| 1 | 692 ms | 133 ms | **122 ms** | **5.67x faster** |
| 2 | 138 ms | 150 ms | **139 ms** | +0.7% (noise) |
| 4 | 1,444 ms | 142 ms | **127 ms** | **11.37x faster** |
| 8 (bounded rejection) | 1,272 ms | 102 ms | 1,271 ms | -0.1% (unchanged) |

Raw alternating samples:

| Cell | Checksum | Node ms | Perry ms |
|---|---:|---|---|
| `fields_one` | `122876400` | `[130, 133, 133, 130, 131, 131, 136, 137, 135, 135, 133, 138, 135, 133, 138]` | `[121, 121, 120, 121, 124, 124, 123, 121, 122, 121, 123, 126, 123, 122, 119]` |
| `fields_two` | `239995200` | `[170, 172, 148, 150, 148, 149, 149, 152, 149, 151, 147, 148, 158, 150, 154]` | `[155, 149, 139, 137, 138, 139, 139, 138, 137, 137, 140, 144, 139, 138, 144]` |
| `fields_four` | `383990400` | `[140, 143, 145, 141, 145, 142, 138, 142, 140, 136, 142, 158, 151, 136, 136]` | `[131, 132, 125, 126, 130, 124, 127, 126, 127, 130, 131, 140, 137, 123, 123]` |
| `fields_eight` | `383980800` | `[100, 100, 100, 106, 104, 101, 103, 100, 102, 101, 104, 107, 107, 102, 102]` | `[1260, 1265, 1268, 1273, 1271, 1272, 1274, 1270, 1269, 1278, 1267, 1274, 1271, 1273, 1279]` |

The exact unchanged canonical source also used 15 alternating pairs:

| Implementation | Raw `write_ms` samples | Median | Sink |
|---|---|---:|---:|
| Node | `[8, 7, 8, 8, 8, 8, 9, 9, 7, 8, 7, 7, 7, 8, 8]` | 8 ms | 9595200 |
| Perry | `[6, 5, 6, 6, 6, 6, 5, 7, 6, 6, 5, 6, 5, 6, 6]` | **6 ms** | 9595200 |

The established two-field and canonical paths therefore show no material
regression. The newly supported one- and four-field cells are faster than Node
on the same protocol.

## Full executable parity sweep

The final 25-cell matrix was also executed once in strict Node/Perry order
after the definitive release build. All 25 write-count/checksum pairs matched.
The intended boundaries remained visible: dynamic/safepointing, polymorphic,
wide-storage, variable-bound, eight-field, and class-id-zero cells stayed on
their previous paths, while eligible static call-free nested cells reached
Node parity.

| Cell | Node | Perry |
|---|---:|---:|
| `key_dot` | 136 ms | 120 ms |
| `key_literal` | 133 ms | 123 ms |
| `key_stable_dynamic` | 139 ms | 2,403 ms |
| `key_alternating_dynamic` | 181 ms | 612 ms |
| `rhs_numeric` | 135 ms | 124 ms |
| `rhs_pointer` | 118 ms | 1,488 ms |
| `rhs_allocating` | 152 ms | 11,196 ms |
| `rhs_call` | 117 ms | 3,246 ms |
| `shape_monomorphic` | 136 ms | 123 ms |
| `shape_two` | 129 ms | 5,350 ms |
| `shape_four` | 113 ms | 3,407 ms |
| `shape_transition_before_loop` | 136 ms | 124 ms |
| `fields_one` | 137 ms | 126 ms |
| `fields_two` | 155 ms | 142 ms |
| `fields_four` | 146 ms | 131 ms |
| `fields_eight` | 104 ms | 1,306 ms |
| `loop_single_counted` | 173 ms | 2,334 ms |
| `loop_current_nested` | 154 ms | 146 ms |
| `loop_stable_local_bounds` | 135 ms | 910 ms |
| `loop_nonzero_start` | 126 ms | 942 ms |
| `storage_inline` | 137 ms | 126 ms |
| `storage_overflow` | 181 ms | 7,315 ms |
| `receiver_anonymous` | 113 ms | 122 ms |
| `receiver_class_instance` | 105 ms | 123 ms |
| `receiver_class_id_zero` | 112 ms | 6,740 ms |

This one-pair sweep is parity/rejection evidence, not a replacement for the
15-pair performance measurements above.

## Generated IR and code size

Function-local inspection of the definitive uncached LLVM module found:

- `fieldsOne`: one generalized guard, one fallback PIC;
- `fieldsTwo`: one generalized guard, two fallback PICs;
- `fieldsFour`: one generalized guard, four fallback PICs;
- `fieldsEight`: no generalized guard, eight fallback PICs;
- 16 generated fast inner blocks across the matrix and zero calls in them;
- zero calls to the old two-field guard from new code.

| Artifact | Baseline | Final | Growth |
|---|---:|---:|---:|
| Matrix LLVM IR | 856,485 B / 21,966 lines | 892,819 B / 22,764 lines | +4.24% / +3.63% |
| Matrix app object | 278,968 B | 284,800 B | +2.09% |
| Linked `main` text | 47,232 B | 49,012 B | +3.77% |
| Matrix executable | 6,302,720 B | 6,302,720 B | 0% |
| Canonical executable | 6,170,504 B | 6,170,512 B | +8 B |

## Validation

- `cargo fmt --all -- --check`
- `git diff --check`
- library-target Clippy completed for both changed crates; a stricter
  all-target run reaches pre-existing denied test lints in untouched files
- GC store inventory self-test and full inventory: 1,161 files, 245 audited
  sites, 65 allowlisted
- codegen native proof regressions: 248 passed
- shadow-slot hygiene: 10 passed
- runtime library tests: 1,460 passed, 3 ignored, 0 failed
- release adversarial corpus: exact Node parity in default,
  `PERRY_DISABLE_CLASS_FIELD_INLINE=1`, `PERRY_TYPED_FEEDBACK=1`, and
  `PERRY_VERIFY_TYPED_INTACT=1` modes
- full gap driver: 387 parity passes, 0 compile failures, 0 crashes; the seven
  reported mismatches were all recognized by the driver as already
  known/triaged, and it finished with `Gap gate OK`
- the new `test_gap_6812_object_write_loop_generalization` passed in that full
  release-driver run; its final expanded inherited-setter/Proxy corpus also
  passed a focused release-driver rerun
