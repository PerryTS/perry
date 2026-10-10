Make `Function.prototype.bind` creation smaller for untouched compiled functions.
Their bound objects retain only the target, receiver and argument slots; name
and length come from immutable body metadata when read. The existing shared
closure allocator initializes these slots in one no-collect birth, rooting
only on allocation fallback. Fixed births retain the nursery's initial
pointer-bearing layout without generic slot classification. Bind shares the
call dispatcher's live-body admission. This removes eager metadata calculations,
receiver coercion, handle installation, duplicate type probes and capture initialization
from ordinary constructor self-binding, without a new cache or side table.

Observable metadata getters and overrides keep spec-ordered snapshots, and
the resolved call/apply bound layouts remain unchanged. Add coverage for
compact allocation size, tracing, metadata snapshots, bound construction,
`instanceof`, `toString` and prototype patching after warm-up.
