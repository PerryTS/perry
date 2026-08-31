### Performance

- **Building objects with computed keys got ~10% faster.** The dynamic-write
  miss handler tried the learned shape-transition fast path only after
  opening a handle scope and rooting all three operands — bookkeeping the
  transition path immediately redid for itself. The shortcut now runs first,
  through a value-returning form that roots internally and hands re-rooted
  operands back on the rare miss-after-allocation, so the hot
  fresh-object-construction lane pays one scope instead of two
  (`{}` + 20 computed fields: 48 → 44 ms; pre-interned keys: 43 → 39 ms).
