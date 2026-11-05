String trimming now finds trailing whitespace from the end and derives the
result's UTF-16 length from the removed edges. Repeated long trims reuse the
last immutable source/result pair under a 32 MiB capacity budget, avoiding
repeated interior scans and copies. Cold or evicted trims still copy the
retained bytes required by the flat string representation.

Both cached pointers participate in moving-GC root scanning. Copying re-reads
a rooted source after allocation; unchanged foreign strings are still copied
to preserve their external lifetime contract. Regression coverage includes
ECMAScript whitespace, lone surrogates, malformed WTF-8, append aliasing,
cache bounds, and actual nursery relocation. Unicode indexing is unchanged.
