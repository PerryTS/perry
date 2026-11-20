### Fixed

- **A `RegExp`'s `flags` string could be stored already-stale, permanently.**
  `js_regexp_new` allocates the canonical flags string, then calls `gc_malloc`
  for the header — a collection point — and then writes the *pre-collection*
  flags pointer into `flags_ptr`. An evacuating minor inside that `gc_malloc`
  moves the string, so a live `RegExpHeader` ends up holding a retired
  from-space address for the rest of its life.

  The pattern string was already rooted and re-read across exactly this
  allocation, with a comment explaining why; the flags string was not. Now both
  are.

  This is the root cause of the `lookup_fancy_regex` cluster in #7341 — 5
  catches reaching one read (`string_as_str((*re).flags_ptr)`) from four
  different callers. Worth recording what it was *not*: a no-move window inside
  `lookup_fancy_regex` closes 0/5, and rooting the search-value operand in
  codegen closes 0/5. The helper and the call site were both innocent.

  4 of the 5 cluster tests are now clean and a 6-line reproducer goes 6/6 → 0/6.
