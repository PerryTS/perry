Cover the function-receiver shape-site follow-ups (#12227) and make worker ownership a per-compile input (#12222).

- Remove the compiler-process `PROGRAM_HAS_WORKER` and `PROGRAM_HAS_THREAD_AGENTS` atomics and their setters. Module and function contexts now derive TLS emission, literal preparation, and immutable-global transfer from `CompileOptions`; object-cache keys consume the same options. Concurrent compiles of all four worker/agent combinations must reproduce their serial LLVM IR, with live checks on concat cells and initialization guards.
- Add Node parity coverage for warmed function sites: changed prototypes, throwing class receivers, async/generator receivers, borrowed builtins, proxies, call/apply overrides, and an own Array.prototype.push method. Count implicit-own-key refusals in the existing method-site statistics and test that counter.
- Keep the lightweight prime suppression scope: an isolated old-`gc_suppress` control at the global bootstrap has identical collection counts and trigger states on Zod ×5000 and a bind-once allocation micro, under both generational and full-only GC. The control is checked live at bootstrap in both programs; the micro also witnesses a suppressed first function-prototype prime. Zod builds its global before that prime. Counts (full/minor) are respectively 0/75 and 0/7 with generational GC, and 161/0 and 13/0 with full-only GC; the first trigger states match byte-for-byte and the rebaselined arena trigger remains 128 MiB. Both builds bootstrap at 4 MiB of arena capacity, below the non-generational fresh-block guard; priming does not consume the pre-suppression snapshot through a trigger bump, and later JSON windows take their own snapshot.
- The first-bind materialization footprint is not solely a huge-page step. A controlled first-bind/no-bind probe grows median RSS by 4,724 KiB normally and 3,600 KiB with THP disabled. AnonHugePages grows by one 2 MiB unit normally and stays zero with THP off; base-page smaps attributes the remaining growth to the builtin graph and newly touched bootstrap code.

Validation on qb6: the new gap fixture equals Node 26.5.1 on both the supplied main and the changed compiler, with method sites enabled and disabled. All four real-program drivers also equal Node on both compiler arms, including worker_heavy and buffer_heavy (both pass on this base).

Instructions:u/RSS medians of three interleaved runs per arm, on CPUs 0–55 with ASLR disabled and no measurement lock:

| Program | Main instructions | Changed instructions | Delta | Main → changed RSS (KiB) | Full/minor GC (separate diagnostic run) |
|---|---:|---:|---:|---:|---|
| Zod ×5000 | 7,535,898,993 | 7,536,253,736 | +0.0047% | 36,552 → 36,552 | 0/75 → 0/75 |
| tsc ×3 | 26,001,548,929 | 26,003,449,474 | +0.0073% | 234,044 → 234,048 | 1/4 → 1/4 |
| worker_heavy 4/400 | 2,232,429,400 | 2,218,607,635 | −0.6191% | 133,816 → 126,868 | 46/0 → 47/0 |
| buffer_heavy | 10,485,093,588 | 10,485,185,532 | +0.0009% | 81,224 → 81,216 | 36/2 → 36/2 |

Same-binary instruction spreads are at most 0.013% for the serial programs and 0.776% for worker_heavy. The worker driver emits byte-identical LLVM IR in both arms; task distribution changes heap occupancy and collection counts. With THP off its RSS comparison reverses sign (111,084 → 111,940 KiB), within the same-binary RSS ranges (main 109,032–113,068; changed 111,332–121,136 KiB). No real-program delta exceeds these measured noise ranges.

The complete release codegen suite passes 2,601 tests (six ignored), including concurrent compile determinism; the runtime suites pass 32 method-site tests and 80 function-filtered tests; the new object-cache-key unit test passes. The repository file-size lint has two pre-existing violations at the supplied base: `expr/property_get.rs` (2,012 lines) and `object/delete_rest.rs` (2,002 lines); neither file changes here. Node-version consistency and whitespace checks pass.
