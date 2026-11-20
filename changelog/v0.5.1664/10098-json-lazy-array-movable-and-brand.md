### Performance

- **Lazy JSON arrays are collectable by a minor, and indexed reads no longer re-classify
  the receiver (#10098, #10118).**

  Two independent costs on the same object. A lazy cluster was born into old-gen and
  pinned there, so a **dead** one held its whole element graph live through the
  remembered set until a full collection — which on `records_array_16k:scan` never
  arrived: the arena rebaselined its own trigger 134M->268M->536M->1073M while
  `old_in_use` climbed past 48 MB, and every minor reported `survival_permille=996`,
  `copied_objects=0`, `freed_bytes=0`.

  `GC_TYPE_LAZY_ARRAY` was pinned for two concrete reasons, both removed the way
  `GC_TYPE_REGEXP` removed its own: `json_tape_store` keyed a tape by its owner's
  address (now rekeyed through `json_tape_store::owner_moved` +
  `GcMoveHookKind::LazyArrayTape`), and the copying minor's flip ran no per-object
  finalize hook, so a header dying young leaked its tape (now
  `finalize_dead_copied_minor_from_space_lazy_tapes`, wired in beside
  map/set/errors/regex). The cluster's generation is still decided ONCE, by cache size
  against the pointer-bearing threshold, so #7546's rule that header, cache and bitmap
  share a generation holds; large clusters stay old exactly as before.

  Separately, the indexed inline cache's brand check rejected `GC_TYPE_LAZY_ARRAY`
  outright, forcing every read through four layers of re-classification. The brand test
  now decides on the **tag alone**, with the kind and index guards moved into their own
  block, and the shared pointer proof is computed once in the entry block instead of per
  tier.

  | row | before | after | vs Node | vs Bun |
  |---|---:|---:|---:|---:|
  | `records_array_16k:scan` peak RSS | 205 MiB | **51 MiB** | below | below |
  | `records_array_1m:scan` peak RSS | 159 MiB | **75 MiB** | below | below |
  | `records_array_16k:scan` CPU | — | **-10.1%** | | |
  | `records_array_1m:scan` CPU | — | **-5.5%** | | |
  | `records_array_20m:repeat` | — | — | | **0.93x** |

  The access window showed zero separated regressions.
