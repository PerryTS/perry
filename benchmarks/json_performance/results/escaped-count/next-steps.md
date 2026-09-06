Next experiments, not implemented or measured. The current paired matrix
confirms small-record stringify +1.60%, empty/tiny/small/1 KB object parse
+2.44%/+0.95%/+0.56%/+0.58%, and tiny stringify +0.89% versus empty-parse.
The scalar direct-output checkpoint had smaller parsing costs; preserve its
evidence when investigating generated code placement and common-path checks.

1. Recover the small-record stringify regression introduced by escaped output.
The first escaped-output paired run regresses 2.70% vs empty-parse, and both
escaped candidates use about 2.8% more local process instructions on this row.
The current local sample has 2,439 main-thread samples: 619 in string_piece,
408 directly in emit_record, 310 entering the toJSON absence probe, 166
directly in emit_piece, and 54 entering final string allocation. These are
sampling diagnostics, not accepted CPU percentages. Existing generated code changes: emit_piece 78 -> 193 instructions,
string_piece 329 -> 357, emit_record 836 -> 863. A third Piece discriminant
adds checks to common Inline/String length and emission paths; the escaped
writer is inlined into emit_piece. This is a concrete hypothesis, not proof
that code size or the extra discriminant alone explains the CPU regression.

Consider representing both escaped and unescaped strings as one String plan
(source byte length, output byte length, output UTF-16 length), retaining only
the original String/Inline Piece variants. The output length determines whether
escaping is necessary. Outline the actual escaped byte loop, then rederive the
source once before either copy or escape emission. Keep all u32 overflow checks,
short-string fallback semantics, UTF-8 validation for new eligibility, and the
input/root/prototype/output allocation ordering. Inspect generated common arms
and measure full rows plus paired regressions before acceptance. Do not revive
the rejected Piece-borrowing experiment on the assumption copies must be costly.

2. Tiny-object parse remains roughly 8x Bun in the published checkpoint.
Investigate a complete-input leaf for one primitive field: native key/scalar
planning before pending collection, then existing rooted key/shape cache reuse
and only final fresh-object allocation. Any unsupported key, duplicate, value,
escape, trailing input or cache miss should decline to the current general
parser. Do not retain input pointers across collection. Validate current shape
ids and rederive keys after allocation; preserve ordinary-object marking, cache
bounds, debt, pressure scheduling, outer suppression and moving-GC behavior.
A remembered pointer or shape id alone is not a lifetime proof.

3. Wide-object parse rebuilds 50k key strings because PARSE_KEY_CACHE clears
above 4096 and PARSE_SHAPE_CACHE has a 4096-key budget. Never merely raise these
bounds: they previously retained obsolete key graphs and increased GC work.
A bounded weak cache of existing shape ids could potentially reuse only keys
already alive through real results. Validate descriptor liveness, immutable
shared keys, ordered byte equality, ids after retirement, owner-thread teardown,
GC pointer relocation and duplicate/property mutation behavior before choosing
an implementation. GC suppression during parse does not prove validity at entry.

All 38 CPU, 74 peak-RSS and 36 retained-RSS targets and all earlier regressions
remain requirements. GC production policy stays unchanged from 0.5.1520.
