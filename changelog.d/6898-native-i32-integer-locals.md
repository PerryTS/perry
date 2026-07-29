Native-i32 residency for integer-valued locals seeded by a possibly-out-of-bounds INT typed-array element read — the bcryptjs `_encipher` Feistel accumulators `l`/`r` (`let l = lr[off]`, then only bitwise-updated). They are logically int32 but were stored as f64, paying an `fptosi`/`sitofp` round-trip on every bitwise access.

A new whole-function use-analysis (`collectors/int_valued_ta_locals.rs`) admits a `let`-declared local to `integer_locals` — unlocking the existing i32 shadow-slot + i32-chain lowering — only when BOTH hold, which is what keeps it sound (an `Int32Array` OOB read is `undefined`, not `0`, unlike `Uint8Array`):

- **every write** is i32-producing: an int-kind typed-array read (`Int8/Uint8/Uint8Clamped/Int16/Uint16/Int32`), a bitwise op, `~`, `Math.imul`, an i32 literal, or a `Uint8Array`/`Buffer` byte read — NOT additive `+`/`-`/`*` (i32 overflow; this is why `_encipher`'s `n` stays f64), and not a copy/call/anything else; and
- **every observation** is in a `ToInt32`-coercing context — a bitwise operand or the value stored into an int-kind typed-array element — NEVER where `undefined`-vs-integer is distinguishable (array index, additive operand, comparison, call argument, `return`, `console.log`, `String()`, `typeof`, plain-array/field store, …).

Under those constraints the value is always fed through `ToInt32` (`ToInt32(undefined) == 0`) and the i32 slot is seeded with the same `0` for an OOB read, so the i32 and f64 representations are byte-for-byte indistinguishable. Deliberately conservative (params, `++`/`--` targets, closure-captured locals, and copy chains are excluded); no fixpoint required.

Two supporting codegen fixes in `stmt/let_stmt.rs`: (1) a possibly-non-finite i32-slot init is seeded with the NaN-safe `toint32_wrap` (`ToInt32(undefined) == 0`) instead of a raw `fptosi` (LLVM poison — `0` on aarch64 but a garbage sentinel on x86-64), while known-finite inits keep the cheaper `fptosi`; (2) an `Any`-typed proven-integer local is refined to `Number` when the structural refiner can't type it, so it takes the numeric Let/LocalSet lowering (no `Any` boxing, no GC shadow-slot tracking) and `-O3` can collapse the residual round-trips.

Gated by `PERRY_INT_VALUED_LOCALS` (default on; `=0`/`off`/`false` disables), keyed into the object cache.

`enc.ts` (2.1M `_encipher` calls, quiet M1, min-of-9): 1347 ms -> 1010 ms (1.33x), byte-exact (`lr0=2135713266 lr1=-1949122846`); optimized `_encipher` `fptosi` 8 -> 0. New gap test `test_gap_int_valued_ta_locals.ts` pins the OOB-observability boundary (eligible accumulator byte-exact even on an OOB init; ineligible sibling still observes `undefined`), verified with the flag on, off, and under `PERRY_GC_FORCE_EVACUATE=1`.
