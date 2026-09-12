### Fixed

- **Repeated `JSON.parse` of a wide object held 5x the memory it needed (#10123).**
  Peak RSS on `wide_1m:parse` (a 50,000-field document, 64 parses) was **220 MiB against a
  live set of ~0** — Node holds 136 MiB and Bun 71 MiB on the same workload.

  `arena/allocators.rs` already states the failure mode: a large pointer-bearing object is
  stamped `GC_FLAG_TENURED`, and a minor never sweeps old-gen, so its cost "is not its own
  bytes, it is every object it can reach, held live through the remembered set by a container
  nothing refers to any more". A wide document's **property storage** and its **shape-keys
  array** are exactly that container. Above 16,384 fields each crosses
  `LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES` (128 KB), is born tenured, and then holds its
  whole field or key set live long after the document itself is dead.

  Confirmed to the byte with `PERRY_GC_CENSUS`, fixtures straddling 131,072 bytes at 64 parses
  (retained shape-keys arrays / live / peak RSS):

  | fields | keys array | retained | live | peak RSS |
  |---:|---:|---:|---:|---:|
  | 16,300 | 130,416 B | 0 | 0.0 MB | 29 MiB |
  | 16,500 | 132,016 B | 30 | 22.7 MB | 98 MiB |
  | 50,000 | 400,016 B | 15 | 34.3 MB | 177 MiB |

  The census named it outright: `shape_keys_arrays {count: 15}`, `slot_tags {string: 750000}`
  — 15 arrays x 50,000 keys = 750,000 live strings. The same binary under `PERRY_GEN_GC=0`
  reports **336 bytes live**.

  Fixed by admitting a wide JSON document's own storage into the nursery past that threshold,
  for as long as the copier can still move it (`JsonWideBirthScope`, ceiling 512 KB — half a
  nursery block, inside `copying::MAX_YOUNG_MOVE_BYTES`). The check is reached only from the
  cold large-object branch of `arena_alloc_gc`, behind a short-circuiting `&&`, so no
  allocation hot path gains work.

  **`wide_1m:parse`: 220 MiB -> 40 MiB and 0.20s -> 0.15s** — below Node's 136 MiB and Bun's
  71 MiB, and faster than before.

  **Scoped and type-masked deliberately.** Raising the constant globally reaches the same
  40 MiB, but it also moves ordinary ARRAY element storage into the nursery, which other rows
  do not need and cannot afford: `records_array_8m:scan` 643 -> 710 MiB with CPU 0.91s ->
  1.20s. Only a document's own object storage and its keys array are admitted. Measured in a
  single binary across `records_array_8m:scan`, `records_array_20m:parse`,
  `records_array_1m:scan`, `records_array_16k:scan` and `heterogeneous_1m:parse`, the scoped
  change is **byte-identical to baseline on every one of them**.

  Three tests that hardcoded a field count and then asserted `pointer_in_old_gen` now derive
  their width from the governing ceiling, so the old-gen path stays covered whatever that
  constant is — previously they silently depended on it being 128 KB and failed on their
  premise rather than their subject.

- **Large JSON record arrays are allocated once, at their estimated size (#10123).** The
  direct parser already pre-sized `[{...}]` arrays from `remaining_bytes / 96`, but clamped
  the estimate at 16,384 slots — a 131,088-byte allocation, **16 bytes over** the
  131,072-byte pointer-bearing birth threshold. Every large record array was therefore born
  old on its very first allocation and then doubled twice more in old-gen (131 → 262 →
  524 KB for 59,000 rows), and an old array of young records keeps them alive through the
  remembered set after the document dies. On `records_object_8m:parse`
  `remembered_set/array` was the origin of 98% of minor survivors, across three minors and no
  full collection.

  The estimate is now used as-is: one allocation, admitted into the nursery when it fits the
  JSON young-birth ceiling (raised to 768 KB, three quarters of a nursery block, so a 7.1 MB
  document's 593 KB estimate qualifies) and born old in a single allocation past it.
  Measured in one binary against the previous clamp, all 50 matrix cells:

  | row | before | after |
  |---|---:|---:|
  | `records_object_8m:parse` | 187 MiB / 167 ms | **118 MiB** / 206 ms |
  | `records_array_20m:parse`, `:scan`, `:sparse` | 256 MiB | **240 MiB** |
  | `records_object_20m:parse` | 256 MiB | **240 MiB** |

  `records_object_8m:parse` reaches parity with the better of Node and Bun on RSS (1.68× →
  1.05×) while its CPU stays at 0.88× of the better engine. No other cell moved outside noise.
