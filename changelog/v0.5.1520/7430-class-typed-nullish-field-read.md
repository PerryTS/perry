Fixed a field read through a class-annotated binding holding `undefined`/`null`
silently answering `undefined` instead of throwing `TypeError` (#7153) — e.g.
`short[5].id` on a `Row[]` where index 5 is out of bounds, which Node aborts
with `Cannot read properties of undefined (reading 'id')`. The class-field
guard diamond's fallback (value-context and raw-f64 number-context lowerings,
plus the outlined `js_class_field_get_ic`) now mirrors the generic dispatch
path's nullish-receiver check on the cold fallback arm; the fast path is
unchanged. Covered by `test_gap_7153_class_typed_nullish_field_read.ts`.
