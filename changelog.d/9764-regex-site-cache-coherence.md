Fixed the RegExp construction cache remembering an incoherent program pair. The
compiled-program caches are capped and cleared independently, so `FANCY_CACHE`
can drop a lookbehind/backreference pattern while `REGEX_CACHE` still answers
for it with the never-match placeholder. The maps heal on their next clear; a
memoized entry does not, because a construction that hits it is born built and
never consults the maps again — so a lookbehind literal could be born
permanently non-matching. The triple is now only remembered against the text
when it is coherent.
