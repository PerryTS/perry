Two dead-work removals on the property read path. The computed-key read loop
now **matches node** (23 ms vs 23 ms on the same host), and the pure property
read drops 21 → 17 ms.

**1. Typed-feedback observation is skipped when recording is off.** Recording
is off by default, and `guard_observe` and `record_fallback_call` both
early-return in that mode — but the property wrappers built the whole
`Observation` first, hashing the key and resolving the receiver's shape, purely
to hand it to functions that discard it. `js_typed_feedback_object_get_field_by_name_f64`
was 10% of an isolated property-read loop, nearly all of it that. The array
index wrappers have carried #5094's gate for exactly this reason, and #8951
gave it to the fast store path; the property get/set wrappers never got it.

Behaviour is unchanged in both modes: with recording off `guard_observe`
returns `contract_valid` unmodified and the fallback recorder is a no-op, so
the wrapper already reduced to precisely the underlying call it now makes
directly.

**2. The slot bound stops copying a descriptor to read four bytes.**
`shape_descriptor_by_id` returns `ShapeDescriptor` **by value**, so
`object_live_slot_count` — consulted on essentially every property read and
write — lifted the whole ~48-byte record and kept only
`live_inline_slot_count`. It now reads that field through the table's record
using the same way-cache probe and the same epoch validation.
`shape_descriptor_by_id` was 10.1% of the same loop.

Interleaved A/B, min-of-21, node on the same host in brackets:

| loop | base | this PR | |
|---|---|---|---|
| pure property read | 21 ms (4) | **17 ms** | −19% |
| computed-key read | 27 ms (23) | **23 ms** | −15% — now at parity with node |
| combined overwrite | 46 ms (31) | **41 ms** | −11% |
| write only | 21 ms (23) | 21 ms | unchanged; already faster than node |

Suite 2779 passed (including the 55 typed-feedback tests). Private-member
output is byte-identical to base; computed-key differential is byte-identical
to node.
