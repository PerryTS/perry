- Fix: `Array.from` and `Array.of` held their result, source, callback,
  snapshot and iterator as raw values across the mapping callback, getters,
  iterator steps and constructor calls, all of which can run an evacuating
  minor. Every later element was then stored into from-space; the result came
  back with holes and the stores corrupted whatever the nursery placed there
  next, which a later minor met as `[gc-pin-latch] FATAL` (OpenCode TUI,
  `Array.from` in `integration.ts`). All operands now live in runtime handles
  for the whole algorithm, and the plain and Proxy paths share one
  `CreateDataProperty` / `Set(length)` implementation.
