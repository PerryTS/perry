# Runtime claim diagnostics

Compile with `PERRY_FACTS_UNITS=0 PERRY_VERIFY_FACTS=1` to emit existing claim witnesses and entry traps for bodies the activation pass classified as known inactive. Unknown, unseen and over-budget bodies remain unrestricted. Successful activation claims add no instruction to active bodies.

A disagreement writes one escaped UTF-8 JSON line to file descriptor 2 and calls `_exit(86)`. Its marker is `PERRY_FALSE_PROOF`. Fields identify the source module, byte offset, key, verdict, predicate, proven receivers, observed receiver/member/callee, emitted copy and compiler commit. An activation report has key `<entry>` and no receiver. Metadata is image-resident bytes supplied directly with its length, independently of JavaScript string initialization.

The adjacent `<output>.facts.jsonl` file records coverage at compile time. It distinguishes operative consumed predicates, solver-proven site claims and the static-call miss sites that also exist in normal builds. Solver identities use their original ordinals and body contexts, including sites whose generic guards remain. Each record names an emitted check, the slice that must supply its check, or the absence of any live use of its failure metadata after lowering and optimization. A slot-key check does not count as checking data attributes, byte offsets or lanes.

Copy identities include both the original lowering and the final LLVM body and call ordinal. A mode-only walk assigns static metadata after LLVM inlining and cloning; joined metadata keeps each pointer paired with its new length. Passing-path witness counts can be printed at exit with the existing `PERRY_CONTRACTS_REPORT=1` runtime control.

This infrastructure checks the existing predicates and known-inactive body entries. It does not implement the uncovered predicates in the manifest. A run observes only the paths it executes and stops at the first disagreement.

With the mode unset or equal to `0`, the compiler performs no diagnostic inventory or metadata construction. Normal compilation retains its existing static-call miss helper and its existing behaviour. Diagnostic mode uses separate runtime entry points; normal binaries must not link them.

Field-kind, receiver and slot checks have reduced runnable program witnesses. No program on main reaches the call, call-miss or inline-miss reporters with a false claim at S0; direct runtime unit subprocess controls cover those reporting paths. Their manifest records state this validation scope explicitly.

The executable controls retain thirteen reduced programs whose claims were fixed on main as Node-output regressions. Explicit receiver-set and field-kind reporting controls use the existing compiler claim switches; inactive-body controls cover call, accessor, `this` and native escape paths. Verification uses the serial diagnostic callable-lowering path to keep source and copy metadata on its owning thread.
