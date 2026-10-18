Subclasses whose admitted family parent belongs to another module can now receive static prototype shapes. Prototype birth checks the actual family parent and canonical parent prototype before requesting the compiled shape, falling back to ordinary birth when either proof fails. This enables guarded direct calls to an imported subclass's own methods while preserving prototype mutations and evaluated heritage.

Evaluated declarations with a structural run-once source proof and a closed loader/cache capability census use canonical class birth. Repeated definitions and possible CommonJS reloads retain per-evaluation identity; capture, static-initializer, computed, private and decorated state retains the fresh path. No runtime flag, table or observation is added.

Local evaluated heritage retains its ordinary birth path; its prototype admission remains a separate follow-up.
