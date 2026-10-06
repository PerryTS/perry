Document the WeakMap performance lane's owned-index redesign instead of
landing a partial lookup optimization that retains the address-keyed index.
The design describes Shape-owned collection branding, scoped validated
lookup views, collection-owned buckets, moving-GC rehash ordering and the
ephemeron correctness tests required before implementation. This change
does not alter runtime behavior or claim a passing performance gate.

Retain measured full-static baselines, WeakMap/Effect DWARF attribution and
verification results in the design. A new diagnostic witness confirms that
baseline keeps a weak key alive when the entry value references that key;
the collector protocol must address this before the storage migration.
