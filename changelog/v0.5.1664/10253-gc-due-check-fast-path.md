Made the GC's "is a collection due?" check cheap when nothing is due, without
changing any collection decision. Runtime safepoint polls (regex search quanta,
the microtask pump, the event loop) no longer build a step result whose debt
snapshot nobody read. The young-generation occupancy the nursery cap compares
against is now O(1): the bytes outside Eden's current block are cached per heap
generation, and every move of an arena's current block goes through
`Arena::set_current`, which invalidates the cache. Debug builds check every cached
answer against the block walk. `gc_check_trigger`, reached from every `gc_malloc`
and JSON parse, evaluates the due trigger once instead of up to three times.
Measured at the shipping release profile: 1M hoisted `re.exec` −12.5 %
instructions, 400k small `JSON.parse` −12.3 %, 1M hoisted `re.test` −6.6 %,
with async, allocation, string, BigInt and Map workloads unchanged and peak RSS
unchanged.
