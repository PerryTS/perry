# R26 access profile: remainder and generated field work

These are sampling diagnostics of the exact frozen R26 access worker. They are independent of R29 stringify. The full identical loop count and warmup were run under Node 26.5.1; checksum and KEEP agree. The access worker does not emit complete JSON. CPU and RSS of sampled processes are not benchmark evidence; inclusive percentages overlap and cannot be added.

| Mode | Workload samples | Dynamic remainder inclusive | fmod inclusive | Generic index helper inclusive |
|---|---:|---:|---:|---:|
| fields | 690 | 13.04% | 11.74% | 11.88% |
| random | 731 | 54.17% | 51.44% | 8.89% |

The modulo belongs to workload index arithmetic (i % length, or (cursor * 17 + 7) % length), through js_dynamic_mod. It is separate from canonical_u32_index and lazy materialized-array reads. Most other field-walk self samples are in the generated run function; named property-lookup/root/GC groups showing zero do not establish zero inlined cost.

Next proposal: inside the existing both-plain-double branch, accept exact nonnegative u32 dividend and exact positive u32 divisor, preserving negative zero via fallback. Full u32::MAX is valid here. Preserve fmod for all other numbers and the existing coercion/BigInt paths. The standalone numeric model is separate evidence, not production timing.
