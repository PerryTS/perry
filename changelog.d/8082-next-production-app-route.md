### Fixed

- Preserve production Next.js App Route request state through generated
  `AppRouteRouteModule.handle` dispatch, imported handlers, async continuations,
  separate runtime providers, and verified moving garbage collection.

- Cap the in-process optimization cost of statepoint relocation fan-out: a
  single 51k-line minified Next chunk closure grew 40x to 2.1M instructions
  under `rewrite-statepoints-for-gc`, and one `-Os` function pass then ran for
  over an hour on it. Post-rewrite, functions past 512k instructions (tunable
  via `PERRY_LL_RS4GC_OPTNONE_INSTRS`) are stamped `optnone`+`noinline` so the
  pipeline skips exactly them; the affected unit now finishes in ~21s. The
  external text path already re-derived its #4880 opt tier from the rewritten
  text and needed no change.
