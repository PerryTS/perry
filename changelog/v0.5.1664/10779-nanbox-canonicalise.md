**A double read out of an `ArrayBuffer` can no longer forge a boxed value.**

A bit pattern falling inside the NaN-box tag window read back as its payload integer rather than the NaN it is, and `Number.isNaN` reported `false` on it. This is memory safety rather than arithmetic: 80 of 144 probe patterns diverge on base and **32 SIGSEGV** — `0x7FF9…` reads back with `typeof === "string"`, `0x7FFD…` as `[object Object]`, a pointer forged out of user-controlled bytes. Seeded GC stress survives **0 of 10 seeds** on base and 10 of 10 after.

Canonicalising float reads out of an `ArrayBuffer` is sufficient, because a field is a conduit rather than a source — perry already enforces the same invariant for `Array<number>` on the store side. The `Float64Array` element read costs **−0.006%**, and no row where perry beats node regresses.

NaN payload bits are no longer preserved through a JS number, matching JSC and SpiderMonkey rather than node. Spec-permitted, and unavoidable under any sound design.
