### Fixed

- **gc: `globalThis` lazy builtin population held a raw pointer across its own
  allocations (#6982).** `js_get_global_this` registers *two* root slots for the
  freshly allocated singleton (`THREAD_GLOBAL_THIS` and `GLOBAL_THIS_PTR`)
  precisely so an evacuating collector rewrites them on a move — and then passed
  the raw, pre-GC pointer by value into `populate_global_this_builtins`, which
  installs several hundred builtins and allocates on nearly every step. When a
  copying minor relocated the singleton mid-population, every later
  `js_object_set_field_by_name(singleton, ..)` /
  `set_builtin_property_attrs(singleton as usize, ..)` addressed the dead
  from-space copy, whose bytes had already been recycled for freshly relocated
  objects. `js_get_global_this` then returned that stale address too.

  The singleton (plus the intermediate pointers in
  `alias_number_static_to_global_function` and
  `alias_typed_array_proto_to_string`) is now rooted in a `RuntimeHandleScope`
  and re-read through the handle at every use, and `js_get_global_this` returns
  the value re-read from its registered cache slot. Binding `singleton` as a
  closure rather than a value makes the conversion exhaustive by construction —
  any use that was not converted fails to compile.

  Only reachable with the conservative native-stack scan off, which is what
  production's `Auto -> SkipDisabled` resolves to; the scan was masking the bug
  by pinning the argument register. This is the same class as #6951/#6972 (a raw
  reference held across a collection point), one layer further out.

  Measured on the #6981 representation corpus (macOS arm64, pinned Node 26.5.0,
  `PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0
  PERRY_CONSERVATIVE_STACK_SCAN=off`, `copied_objects` > 0 on every run):

  - crashes on the evacuating precise-roots arm: **6 -> 2**
    (`repsel_canonical_i32`, `ta_param_numeric_read`, `typedarray_param_read`,
    `repsel_ptr_shape_barriers` no longer crash);
  - `repsel_canonical_i32` flips all the way to **OK** (byte-identical to Node),
    the rest become mismatches — progress, still red, tracked separately;
  - all 21 corpus files remain byte-identical to Node on the as-shipped
    configuration (no regression).

  Removing this shared first hurdle exposed two independent defects that were
  previously masked by it: a compiled constructor's receiver going stale across
  the same collection (`repsel_ptr_shape_locals`, still SIGSEGV) and a lost
  method value (`repsel_proven_this_frozen`, now `TypeError: bump is not a
  function`). Both are filed separately.
