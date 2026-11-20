The dynamic typed-array dispatcher defaulted a missing first argument to
NaN, so `ta.join()` on a receiver whose kind codegen does not know
statically (a `sort()`/`reverse()`/`map()` result, a Uint8ClampedArray local)
printed "NaN" as the separator: `1NaN3` for `1,3` (competitive A/sort-4,
A/typedarray-5). The default is now `undefined`, whose NaN-box is a NaN, so
the numeric callees (`at`, `with`, `copyWithin`, `fill`) are unchanged.
