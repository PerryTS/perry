**perf(codegen):** `const {a, b, ...rest} = obj` built its excluded-key array
(the list of statically-named keys `ObjectRest` must NOT copy into `rest`) via
one `js_array_alloc_with_length` call plus one `js_array_set_f64_unchecked`
call *per excluded key* — each of those per-key calls re-derived and
re-bounds-checked a receiver the site had just allocated itself, so every
check inside (frozen? has index descriptors? index in range?) was statically
true. The excluded keys are compile-time-known string literals, so this is
exactly the "array literal of known values" shape `perry-codegen` already has
a cheap path for (`lower_array_literal`/`emit_array_from_lowered_values`,
previously reused only for the rest/`arguments` call-bundle case): one inline
bump allocation plus N `store double`, with a single call only on the cold
arena-full arm. `js_object_rest` itself, and everything downstream of it, is
unchanged — only how its `exclude_keys` argument gets built changes. The
source object's pointer is now also derived *after* that allocation rather
than cached across it.

Also added `test-files/test_gap_object_destructuring_field_and_rest_guard.ts`,
covering 2-field, 5-field, nested, defaulted, and rest object destructuring
(including computed-key exclusion, an empty pattern before rest, and function
parameter destructuring), plus the easy-to-break edge cases: missing
properties reading as `undefined`, defaults applying only to `undefined` and
not `null`, getter evaluation order following the *pattern's* key order (not
the source object's), and `null`/`undefined` sources throwing `TypeError`
(including for an empty pattern and a `...rest`-only pattern).

Along the way, found and confirmed **pre-existing** (unaffected by this
change, reproduces identically on unmodified `main`): `const {...rest} = obj`
silently drops any Symbol-keyed own property of `obj` from `rest` instead of
copying it through. Not fixed here — it's in `js_object_rest`'s own key-copy
logic, unrelated to how the `exclude_keys` array is constructed — but worth a
follow-up issue.
