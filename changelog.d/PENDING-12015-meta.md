Use seeded ConstFn shapes as the authority for completed method-record
publication. Reuse their key prefix and immutable body metadata, while checking
the receiver's current closures and retaining bootstrap validation for unseeded
reconstruction. Compiled-body ABI facts avoid redundant native admission probes.

Read ordered key and slot bounds from borrowed shape records. Internal key
reads honor shape prefixes, consumed fronts and holes without JS array probes.
Object.keys uses shape-owned non-enumerable metadata; effective class descriptor
resolution and values/entries snapshot checks remain intact.

Attribution, parameter-receiver micros, validation progress and measurements:
`docs/perf-12015-attribution.md`. Full validation and A/B results are pending at
this milestone; no performance claim is made yet. Versions are unchanged.
