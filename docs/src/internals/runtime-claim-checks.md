# Runtime claim diagnostics

Compile with `PERRY_FACTS_UNITS=0 PERRY_VERIFY_FACTS=1` to emit existing claim witnesses and entry traps for bodies the activation pass classified as known inactive. Unknown, unseen and over-budget bodies remain unrestricted. Successful activation claims add no instruction to active bodies.

A disagreement writes one escaped UTF-8 JSON line to file descriptor 2 and calls `_exit(86)`. Its marker is `PERRY_FALSE_PROOF`. Fields identify the source module, byte offset, key, verdict, predicate, proven receivers, observed receiver/member/callee, emitted copy and compiler commit. An activation report has key `<entry>` and no receiver. Metadata is image-resident bytes supplied directly with its length, independently of JavaScript string initialization.

The adjacent `<output>.facts.jsonl` file is compile-time coverage evidence. Each consumed predicate has a stable identity, its operands and emitted-copy identity, and either its check or the slice before which its check is not covered. A missing check does not establish correctness. A run observes only the paths it executes and stops at the first disagreement.

With the mode unset or equal to `0`, the compiler performs no diagnostic inventory or metadata construction. Normal compilation retains its existing static-call miss helper and its existing behaviour. Diagnostic mode uses separate runtime entry points; normal binaries must not link them.
