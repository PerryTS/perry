**Fixed: the `gc-root-dominance` nightly, red since #8120 — buffer/typed-array constructors were missing from `POLL_CAPABLE_RUNTIME`.**

`--audit-poll-reach` exists to catch exactly one shape (#7616): a symbol whose
result the checker tracks as a heap value (`ALLOC_RE`) that calls something
already known to re-enter JS or run a moving minor, without itself being
listed. A window whose only collection point is such a call classifies
`MOVING: no`, so every `--moving-only` arm — which is every gated arm, in all
four corpus × lowering modes — silently drops it.

Five constructors reach an element read that way, and the audit named them
with their edges:

```
js_uint8array_new             -> js_typed_array_get, js_uint8array_from_array
js_typed_array_new_from_array -> js_array_get_f64
js_buffer_from_array          -> js_array_get_f64
js_buffer_from_value          -> js_buffer_from_array
js_buffer_alloc_fill_value    -> js_buffer_from_value
```

The reads are the reach proof: a source element can be an accessor or a Proxy
`get` trap — user JS — and the per-element loop allocates the destination as
it goes, so `Buffer.from(arr)` / `new Uint8Array(arr)` are collection points
like any other element-reading builtin. All five are now listed, which both
greens the audit and un-drops the windows they gate.

Validated locally against built runtime archives: `--audit-poll-reach` goes
from exit 2 (five pairs) to exit 0 (`no ALLOC_RE symbol reaches a poll-capable
one unlisted`), converging in one pass — listing these five surfaces no
further unlisted callers.

Widening this set is one-sided: it can only make windows VISIBLE that the
`--moving-only` arms previously dropped, so a corpus arm can newly report a
finding it was blind to. That is the point of the change, and it is also the
way it could newly exceed a budget — the four corpus × lowering gates in CI
are what confirm the budgets still hold.
