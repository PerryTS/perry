# Class fast-path witnesses (#12372)

These are the issue's four standalone TypeScript micros and its exact stripped
JavaScript equivalents. Keep argument parsing, ten 20,000-iteration warmups,
measurement markers and checksums when including them in a performance lane.

| Witness | Exercises | Checksum at N=200,000 |
| --- | --- | --- |
| class_getter | typed getter/setter and numeric own field | `77881568:77792480` |
| object_array | typed class records in an indexed array | `77881568:77792480` |
| class_virtual | alternating base/derived virtual calls | `103875424:103756640` |
| object_spread | spread/destructuring control | `128869280:128720800` |

Build compiler and both static wrappers in an isolated Linux target; set
`PERRY_RUNTIME_DIR` to that target's release directory. Compile each `.ts` with
`perry compile <source> -o <binary>`. The Node oracle is `.node-version` and the
matching `.js` file. Compare markers/checksums, excluding only `elapsed_ms`.

For instructions, take five interleaved samples at N=50,000 / 100,000 / 200,000
with ASLR disabled. Report the median paired slope `(high - low) / 150000`;
100,000 checks linearity. Linux `perf stat -e instructions:u` counts retired
user instructions over the process; subtracting scales cancels startup and
warmup. The issue's Windows DynamoRIO region counts are a different metric,
so do not impose their absolute budgets on Linux. Report emitted `.text` too.

Cheap regression ratchets run in the codegen field-initializer and runtime
class-layout unit suites: ordered definitions must preserve the completed
numeric ShapeId, keyless reservations must fail admission, and a contradictory
store must revoke it. `test_gap_class_completed_layout.ts` checks observable
undefined defaults, key order, inheritance, class expressions and mutations.
