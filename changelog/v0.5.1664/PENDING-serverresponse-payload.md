### Fix standalone ServerResponse retention during fastify inject

Standalone `http.ServerResponse` instances now own a typed native payload through their traced `native_state` word. Their sockets and listeners live in ordinary object-owned JS state, so response/listener/socket cycles are collectible. The constructor no longer registers a global handle or installs the HTTP handle root scanner. End and destroy release native state explicitly; sweep remains the backstop. Observable terminal properties survive explicit release.

Native-this aliases now use one traced forwarding record for both handles and payload objects. An object that is also a stream keeps that record in the stream record's `NativeAlias` slot; neither family overwrites the other's state. Transport responses and bare OutgoingMessage handles retain their existing transport ownership.

Coverage includes captured-response listener/socket cycles, explicit release through every end entry point, global-root sabotage, and both alias/stream construction orders. The compiled alias lifetime test and the reproducible 20,000-response benchmark include once(close), once(error), and socket error listeners capturing the response. The handle ledger loses the standalone producer; its already-stale weakref/index entry is also removed to make the ledger match the tree.

Validation on perrymaster (Node 26.5.1, isolated main/payload targets, release builds, `-j 8`): 202 ext-http unit tests, 120 stream tests, five native-this alias tests, stream-record and payload/stream lifecycle tests, and compiled 20k alias lifetime coverage pass. Deliberately restoring global response roots makes the churn witness fail. The default HTTP and fastify parity subsets have zero changed results versus main; pre-existing HTTP/2 failures and fastify timeout are retained. Stock fastify 5.12.5 inject output equals Node (`b6154b94`), as do tsc, Zod, hello, and both shared buffer/worker drivers.

Performance: medians of five interleaved runs on perrymaster CPUs 0–15, `setarch -R`, `instructions:u` and peak RSS, off the measurement lock. Full-GC counts come from matching diagnostic runs.

| Program | Instructions:u main | Instructions:u payload | Δ | Peak RSS MiB main → payload | Full GCs main → payload |
|---|---:|---:|---:|---:|---:|
| fastify inject ×5000 (+300 warmup) | 52,731,980,760 | 43,499,040,620 | -17.5092% | 226.676 → 74.211 | 1 → 1 |
| response churn ×20000 | 12,717,489,268 | 9,808,217,710 | -22.8761% | 132.500 → 25.848 | 20 → 20 |
| tsc transpile ×3 | 25,968,882,662 | 25,968,936,492 | +0.0002% | 223.809 → 223.750 | 0 → 0 |
| Zod ×5000 | 13,080,009,355 | 13,081,260,343 | +0.0096% | 26.562 → 26.496 | 0 → 0 |
| hello (equal-length executable paths) | 1,168,777 | 1,168,778 | +0.0001% | 16.074 → 16.176 | 0 → 0 |
| buffer_heavy ×12 | 10,434,316,005 | 10,437,337,699 | +0.0290% | 75.340 → 75.344 | 36 → 36 |
| worker_heavy (4 workers, 400 jobs) | 2,185,389,941 | 2,199,929,008 | +0.6653% | 111.227 → 112.344 | 47 → 47 |

The churn's median end-of-batch RSS is 30.398 → 129.379 MiB on main versus 22.137 → 22.422 MiB with the payload (2k → 20k responses); WeakRefs retain 100/100 versus 0/100. Removing growing global roots reduces tracing work and releases native memory; full collections remain 20 per arm. Fastify full collections remain one per arm.

The buffer driver's +0.0290% instruction change is the bounded cost of validating the fixed stream-record layout and rechecking its payload slot now that aliases also use traced arrays. Fix-forward: pass the already-proven GC array type into the slot reader and validate length once, without a cache or latch. Worker +0.6653% is within same-binary instruction spans of 0.842% / 2.238%; its RSS ranges overlap. tsc and Zod differences are also inside same-binary noise. Hello's initial +92 instructions came from the longer executable path in startup string/proc parsing; equal-length hardlinks reduce it to +1 instruction (within a 32-instruction span). No stable RSS delta near 2 MiB remains.
