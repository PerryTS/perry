### Fixed

- **Touching `globalThis` no longer disables the per-object GC layout fast path
  for the rest of the process.** `churn` and `tree` were paying +28% / +29% for
  a single `for…of` in `main()`, and every real TypeScript program was paying it
  too.

  `globalThis` is populated lazily on first touch, and *any* plain-object or
  array property **miss** forces it: the miss walks the prototype chain, reaches
  `builtin_prototype_value` → `js_get_global_this_builtin_value`, and runs the
  several-hundred-builtin bootstrap. Perry's whole benchmark corpus happens
  never to take that path — no `for…of`, no spread, no `Symbol`, no property
  miss anywhere in `churn`/`tree`/`interp`/`shapes`/`asyncpipe`/`retain` — so
  the cost was invisible to every number in the perf campaign while real
  programs paid it before their first line of work.

  It was **not** the ~1.15 MB the bootstrap allocates. GC behaviour is
  effectively identical with and without it: 105 minors on `churn` either way,
  and ~616 KB more copied across the entire run. The cost was a **global latch**.
  The bootstrap builds hundreds of permanently-rooted plain objects, and each
  one's first pointer field minted an entry in the per-object GC slot-layout
  side tables. Those entries are immortal, so `PER_OBJECT_LAYOUTS_NONEMPTY` —
  the emptiness proof that keeps `layout_forget_object` off the allocation,
  death and relocation paths — could never go `false` again. Measured
  `layout_forget_object` self time: `churn` 112 ms → 916 ms, `tree`
  194 ms → 740 ms, which is essentially the whole regression in both.

  This is #7510's lesson at 1000× the scale ("one immortal entry is enough to
  nullify an is-empty accelerator"; there it was a single interned keys array).
  Two changes, and **both are needed**:

  1. `gc::ImmortalLayoutScope` around `populate_global_this_builtins`. Inside
     it, an object that would mint a per-object pointer mask declares
     `GC_LAYOUT_UNKNOWN` instead — the tag-checked payload scan, which is the
     code's own fallback for the same situation and the universally safe state,
     not a weaker one. For an object that is never reclaimed the mask bought
     precision nobody spends, at the price of two `RefCell` round-trips and two
     hash probes on every allocation the program would ever make. Bootstrap
     residue: **1113 entries → 0**.

     Deliberately **not** applied to typed-shape layouts
     (`init_typed_shape_layout`): those describe raw-f64 slots, whose bit
     patterns can alias a heap pointer, and a conservative scan would trace —
     and under the copying collector *rewrite* — a slot holding a number. The
     scope applies only where the mask being replaced is itself derived from
     `layout_pointer_bearing_bits`, i.e. exactly the test `GC_LAYOUT_UNKNOWN`
     re-runs per slot.

  2. An **address filter** replacing `PER_OBJECT_LAYOUTS_NONEMPTY` as the hot
     guard. Change 1 alone moved nothing measurable, and that is the important
     finding: ordinary runtime init still leaves one or two long-lived records
     behind, and for a single global bit two entries are exactly as bad as 1113.
     An 8192-bit thread-local filter over the key addresses turns "is either
     table empty?" into "can this *address* have an entry?", so a nursery
     address the tables have never seen is proved absent in one multiply and one
     load even while immortal records exist elsewhere.

     The filter sits *behind* the flag, and both live in ONE thread-local
     (`PerObjectLayoutHint`). All three arrangements were measured on the quiet
     mini; the co-located one wins everywhere:

     | | `churn` | `push_cls` | `tree` | `interp` | `churn`+`for…of` | `tree`+`for…of` |
     |---|--:|--:|--:|--:|--:|--:|
     | base | 0.422 | 0.356 | 1.627 | 1.888 | 0.539 | 2.151 |
     | filter only (no flag) | 0.438 | **0.383** | 1.673 | 1.934 | 0.500 | 1.857 |
     | flag + filter, 2 slots | 0.421 | 0.368 | 1.642 | 1.950 | 0.506 | 1.886 |
     | **flag + filter, 1 slot** | **0.422** | **0.368** | **1.640** | **1.922** | **0.493** | **1.840** |

     Dropping the flag is a loss: almost every workload is *disarmed*, and for
     those the flag is one load where the filter is a multiply, a shift, a load
     and a test — `push_cls` went past its budget. Keeping both as separate
     thread-locals costs a second `_tlv_get_addr` on exactly the workloads that
     are legitimately armed (`interp`, `iso_miss`). One struct behind the
     existing named hot slot gives the cheap gate AND one resolution.

     `false` is a proof of absence and nothing else rests on it; the filter is
     rebuilt from the live keys once half its bits are set, so a workload that
     genuinely churns per-object records cannot saturate it permanently.

  Measured on the quiet mini (base = `b9415d780`, both arms built locally;
  interleaved, best-of-5, exit-checked): `churn` + `for…of` **0.539 → 0.493**
  (floor 0.422), `tree` + `for…of` **2.151 → 1.840** (floor 1.627). Every
  protected bench stays inside budget. `interp` 1.888 → 1.922 and `iso_miss`
  2.361 → 2.443 still pay the filter test without benefiting from it — they are
  legitimately armed, so it never proves absence for them.

  Gated by five tests in `gc::tests::layout_trace::per_object_tables`, written
  so none of them can pass vacuously: the bootstrap must leave both tables empty
  (with a subject-live check that `globalThis.Array` actually populated); the
  same store *outside* a scope must still mint a mask; an object built inside a
  scope must still trace its children through the fallback scan; a live record
  must survive a filter rebuild and still be found and removed; and — the
  accelerator's own subject-live assertion — the filter must still prove
  unrelated addresses absent *while the global flag is armed*, which is the
  exact condition under which it silently stopped accelerating before.

  `PERRY_GC_DIAG=1` now prints
  `[gc-globalthis-bootstrap] elapsed_us=… per_object_slot_masks=… per_object_typed_layouts=…`
  once per thread, so the bootstrap's cost and its residue are observable rather
  than inferred.
