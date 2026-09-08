### Runtime

- perf(runtime): a `for…of` over an array no longer allocates a `{ value, done }`
  object per element. The fused `for…of` advance (`js_for_of_next`) already
  recycled ONE result object per iterator for builtin Map/Set iterators; array
  iterators fell through to the generic arm and minted a fresh 40-byte object
  for every element. They now take the same fused arm, and the recycling routine
  is one shared implementation rather than a second copy. Manual `.next()`,
  spread, `Array.from`, `yield*` and `for await` are unchanged and keep
  returning fresh results, so a caller that retains one still sees spec
  behaviour; the recycled object is only ever the compiler's own loop temporary.
