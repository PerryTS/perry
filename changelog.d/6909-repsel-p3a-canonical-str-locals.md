perf(codegen): representation-selection Phase 3a — canonical string locals (tagged-at-rest `Str` rep) (#6909)

Phase 3a of `docs/representation-selection-rfc.md`: `SlotRep::Str` marks
function locals proven to hold NaN-box string bits (`STRING_TAG` heap or
`SHORT_STRING_TAG` SSO) at rest. Storage, shadow-slot GC binding, and every
alias/refcount demote stay exactly the pre-phase model (zero GC changes; SSO
stays by-value); the rep is a compile-time proof the string-op lowerings
consume to tag-dispatch inline instead of routing operands through
`js_get_string_pointer_unified` (which heap-materializes SSO and
number-coerces):

- `s += rhs` self-append: both-heap → raw `js_string_append(h,h)` (keeps the
  refcount==1 in-place path), both-string-SSO → `js_string_concat_box`, else
  the exact legacy sequence (annotation lies degrade to today's behavior).
- `.length` on statically-string receivers: SSO inline length-byte extract /
  heap bare `load i32` of `utf16_len` / `js_value_length_f64` cold arm,
  replacing the ~18-op GC-type-byte tower.
- `===`/`<` with a canonical-Str operand: both-heap → direct
  `js_string_equals` / `js_string_compare` on raw handles, else one SSO-aware
  call (`js_jsvalue_equals` / new `js_string_compare_value`).
- `charCodeAt`/`at`/`codePointAt`: proven-heap receiver → bare and-mask
  handle; string-literal operands of coerce-concat unbox inline; `StringRef`
  materialization inlines the `or STRING_TAG` retag (null cold arm kept).

Structural proof on `benchmarks/app-patterns/kernels/string_concat_csv.ts`:
zero `js_get_string_pointer_unified` calls in the emitted module. Gated by
`PERRY_CANONICAL_STR_LOCALS` (default on), keyed into the object cache. Gap
test `test_gap_repsel_canonical_str_locals.ts` covers alias `+=` discipline,
SSO round-trip, lying-annotation acceptance, and non-ASCII/emoji
byte-exactness in all four flag/GC-evacuation arms; `shadow_slot_hygiene.rs`
gains a canonical-Str GC-binding + tag-dispatch structural test.
