Remove EventEmitter's inline-only restriction for stream state operations. The existing shape memo now records the storage kind and admits barriered overwrites of materialized spill positions; unrelated public-field attributes no longer force registration through ordinary property lookup. Descriptors on the state keys and unmaterialized storage retain the general path.

Removing the last unwatched listener uses the same fresh-map reset operation and preserves a retained old map. Once listeners use the same first-listener registration operation after wrapping, and their wrappers receive the native argument vector directly instead of constructing and unpacking a rest array. Arguments remain rooted across an overridden `removeListener`.

Validation includes differential EventEmitter/Readable coverage for once wrappers, symbols, accessor state and meta-events, a spill-refusal negative control, and moving-GC unit coverage for both inline and heap argument roots.

The stream listener micros improve, but Fastify inject is flat in the five-run A/B. This candidate does not meet the Fastify landing threshold; the once argument change has little weight there (15 wrapper calls over the complete driver run).
