`Object.assign` from a plain source copied by position now stores each key
through the shape-answered PutValue funnel (the receiver shape's own slot, or
its learned transition under the shape's prototype-chain fact) instead of
the full named `[[Set]]` tail. The all-keys preflight had already proved
strict `[[Set]]` an own-data overwrite or add for every key, but each store
then re-proved the prototype chain, re-scanned the own keys and re-ran the
exotic-receiver checks (about 4,270 instructions per key on an
`Object.create(proto)` target). The positional loop and the enumerated
loop now share one store (`object_assign_set_string_key`), so the separate
positional `js_object_set_field_by_name` call and its flag are gone.

Instructions:u, n=5 interleaved, `--no-auto-optimize`: shapes-options -24.1%,
Effect -1.59% (45,065 stores moved to the shared funnel), fastify -0.94%
(11,085 stores), schema-record -0.08%; every other program and the four class
micros are flat. GC counts and RSS are unchanged; .text shrinks 64-128 bytes
per program. No code is emitted per site.

Also: the scoreboard "slip" of root-survival (2.28x -> 2.35x node) and
shapes-options (1.87x -> 1.93x) between the 2026-10-11 and 2026-10-11b
scoreboards is a harness artifact, not a code change: the earlier harness
read its program table with a tab `IFS`, which collapsed the empty args
column so `noopt` landed in the args field and the four witnesses were
compiled WITH auto-optimize. Rebuilt d6bbfa03 reproduces the earlier numbers
only with auto-optimize (1.799G / 2.501G / 432.6M for root-survival /
shapes-options / property-prototype); with `--no-auto-optimize` both
d6bbfa03 and current main measure 1.845G -> 1.835G and 2.608G -> 2.599G.
