Probe own numeric, boolean and null fields directly in pristine lazy JSON array
records. Pure-local indexed reads retain ordinary property access on a miss;
escaped ambiguous keys, strings and containers fall back. One short ASCII field
can be memoized in the existing unexposed element slots; zero is encoded without
another allocation. Exposed records remain authoritative. GC continues to trace
only bitmap-selected elements, and collection policy is unchanged.
The probe sits on the dynamic index dispatcher's lazy-array edge. Ordinary
array/object reads keep their established branches. Long cold walks and wide
records fall back to ordinary access; repeated scalar hits use inline reads.
Mixed scalar and object reads retain adaptive batch construction.

Named descriptors and deletions now materialize lazy JSON arrays before applying
ordinary array semantics, preserving indexed getters, replacement values and
holes. Ordinary receiver definitions coerce keys once, before inspecting the
descriptor, with the inputs and converted key rooted across user callbacks.

This is an experimental R2 candidate following R1's rejected reuse regressions;
compiled validation and performance acceptance are still pending. The access benchmark covers repeated, random,
mixed-field and sequential reads separately from parse timing.
