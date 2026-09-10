Probe own numeric, boolean and null fields directly in pristine lazy JSON array
records. Pure-local indexed reads retain ordinary property access on a miss;
cached records, escaped ambiguous keys, strings and containers fall back. The
probe creates no managed objects and leaves collection policy unchanged.
Repeated indices, long walks and wide records fall back to ordinary access.
Mixed scalar and object reads retain adaptive batch construction.

Named descriptors and deletions now materialize lazy JSON arrays before applying
ordinary array semantics, preserving indexed getters, replacement values and
holes. Ordinary receiver definitions coerce keys once, before inspecting the
descriptor, with the inputs and converted key rooted across user callbacks.

This is an experimental candidate; compiled validation and performance
acceptance are still pending. The new access benchmark covers repeated, random,
mixed-field and sequential reads separately from parse timing.
