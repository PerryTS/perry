### Performance

- Thread the stable inline-arena state pointer through directly self-recursive
  allocation functions, resolving it once in an ABI-preserving public wrapper
  instead of repeating the runtime accessor at every recursion level.

- Against `c7645d19b`, retired instructions fall 3.22% for `interp` and 2.16%
  for `iso_miss`. An 11-repeat randomized run on a quiet M1 improves median
  wall time by 1.92% and 1.30%, respectively. All 20 corpus outputs remain
  byte-identical to Node 26.5.1, and the other 18 instruction deltas stay
  between -0.47% and +0.18%.
