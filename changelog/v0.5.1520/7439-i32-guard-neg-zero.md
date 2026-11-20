**Fixed** `js_typed_i32_arg_guard` admitted `-0`, so a specialized-ABI entry and
its boxed twin disagreed on `Object.is(f(-0), -0)`.

`-0` is finite, integral and in signed-32-bit range, so the obvious predicate
lets it through — and `js_typed_i32_arg_to_raw` then maps it to raw `0`, which
the specialized entry re-boxes as `+0`. Every consumer of the guard admits a body
that can return a parameter unchanged, so the fast arm returned `+0` where the
slow arm returned `-0`. There is no `-0` in the i32 domain to round-trip through,
so the guard has to reject it.

Pre-existing on the Tier A path (explicitly annotated `: number` parameters under
`PERRY_SPECIALIZED_ABI`); found while investigating #7287's dead int32 tier.
