# Correction to the earlier indexed-read proposal

The canonical_u32_index integer-roundtrip proposal remains a valid semantics model, but its earlier claim that it would remove the sampled fmod call is incorrect. Exact linked R26 js_packed_arraylike_index_get disassembly uses frintz/fcmp/fcvtzu and contains no fmod call. R24's raw 1 MB fields sample places fmod beneath js_dynamic_mod, called from the access worker's own index calculation. It is not inside canonical_u32_index.

A roundtrip predicate could still reduce the sizeable finite/type/range classification sequence before array access. That is a separate hypothesis from reducing dynamic remainder arithmetic. The 15,525,263-pattern model validates predicate equivalence only and is not a timing result. Do not attribute the old profile's 11.87% fmod share to a future predicate change.

A separate numeric js_dynamic_mod optimization could target common exact nonnegative integers, with strict preservation of -0, negative remainders, NaN/infinity, zero divisor, boxed integer and BigInt dispatch. This would optimize arithmetic in the post-parse workload, not JSON parsing itself. No production change has been made for either candidate.
