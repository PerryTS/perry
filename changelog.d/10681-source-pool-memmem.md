perf(codegen): the over-budget retained-source intern (`share_into_longest`,
#10579) now searches each nested function source in the longest parent with
`memchr::memmem::find` instead of building a single-pattern Aho-Corasick
automaton per needle (#10681). Offsets are unchanged — both return the
leftmost exact byte match.

Measured on `typescript@5.9.3`'s `lib/_tsc.js` (6.2 MB haystack, 10,698
unique nested function/class sources, 17.2 MB total — over the 8 MiB budget,
so this fallback is the path taken): fallback search time 2.43 s → 1.41 s,
with byte-identical `(parent, offset)` results for every needle. The remainder
is the haystack scan itself, not matcher construction. `memchr` was already in
the lockfile (via `aho-corasick`); it is now a direct workspace dependency of
`perry-codegen`, with no new crate versions.
