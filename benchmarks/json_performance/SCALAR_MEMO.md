# Scalar projection R2: memoization and dispatch experiment

Status: implementation under validation; no performance acceptance or PR.
R1's measurements and rejection remain in [SCALAR_PROJECTION.md](SCALAR_PROJECTION.md).
Reference main is `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530).

R1 removed record construction on scalar scans but made every reuse control
slower. R2 addresses both causes: save scalar results and put the probe behind
the existing dynamic index dispatcher's ordinary Array/Object branches.

## Cache contract and consumer audit

`LazyArrayHeader.materialized_bitmap` still exclusively identifies exposed
elements. With a bit set, the corresponding value is the authoritative record
or array element and must preserve identity, mutations, getters and prototypes.
With a bit clear, a nonzero slot can memoize a number, boolean or null for one
fixed property. The header stores that property's exact encoding (length plus
up to seven ASCII bytes), never a pointer or a lossy hash. Another property
takes ordinary indexing. A failed probe does not select a property.

Existing slots start zeroed. The scalar +0 is stored as tagged int32 zero and
decoded back to f64 +0; -0 retains its distinct bits. No additional bitmap,
cache allocation, initialization pass, managed reference or GC policy is added.
The header gains one u64. The inline cache path is limited to 64-bit targets;
the runtime helper uses an explicit u64 length matching its declared ABI.

The complete search of production readers/writers found:

| Consumer | Required behavior retained |
| --- | --- |
| `gc/layout_slot_visit.rs`, LazyArray descriptor | Select slots only by bitmap; memo values do not become GC edges. |
| `json_tape.rs`, `lazy_cached_count` | Count exposed elements only; scalar hits do not trigger batch construction. |
| `json_tape.rs`, `lazy_get` | Read a slot only when exposed; otherwise construct its record, overwrite the memo, barrier it and set its bit. |
| `json_tape.rs`, batch reparse overlay | Overlay only exposed elements, retaining existing mutations and identity. |
| `json_tape.rs`, slow full materialization | Use bitmap-selected values; reconstruct every unexposed record from the tape. |
| `json/stringify_api.rs`, lazy stringify | Only exposure bits force materialization; scalar reads leave original JSON authoritative. Existing canonicalization limitations remain. |
| `json_tape/mutation.rs`, indexed setters/deletion | Materialize before mutation; the materialized pointer closes memo access. |
| `object/object_ops/define_property.rs` | Materialize lazy receivers before installing indexed values or accessors. |

The codegen path checks lazy brand, forwarding (in the existing dispatcher),
magic, materialization, bounds and descriptors before any memo or exposed slot
load. Exposed slots feed the existing property PIC. Scalar hits bypass record
construction and that PIC. Cold scalar reads call the noncollecting runtime
probe; misses keep the ordinary dynamic index helper. IndexGet's specialization
order is preserved by passing an optional probe down to its dynamic tier.

## Acceptance still required

Run runtime/compiler tests, actual compiled Node comparisons under all parser
and GC modes, and static root checks before measuring. Preserve the known R1
native checker finding in accessor-literal construction unless independently
resolved; do not suppress it to qualify this experiment.

Then rerun the same quiet-host scan/parse/stringify and parse-once reuse controls
against freshly built R2, frozen measured main, Node and Bun. A particular risk
is repeated access through a lazy wrapper after full materialization: its
additional checks might still cost more than main's path. Full matrices and
landing are conditional on clearing the targeted regressions first.
