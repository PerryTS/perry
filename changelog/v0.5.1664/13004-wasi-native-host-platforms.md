Add the manually dispatched `wasi-hosts` CI suite to qualify native P1 and typed
P2 hosting on Linux x86_64, macOS ARM64 and Windows x86_64. Build matching compiler,
engine, runtime and instrumented archives, execute both host crate unit suites,
compare the complete P1 suite against the pinned Node oracle, and run the native
P2 component acceptance suite.

Qualification requires the expected exit statuses, matching P1 output, actual
moving collections, evacuation checks and quarantine witnesses. Record source
and artifact identities alongside retained logs. The suite summary rejects
skipped, missing, failed and cancelled platform jobs. This starts as a manual
qualification suite; platform support is not claimed before execution evidence,
and existing required gates are unchanged.

Use an explicitly opened descriptor in the P1 stdio-constructor fixture. A
hard-coded descriptor 3 can name Node's internal event-loop handle but be absent
in Perry, making that comparison depend on process internals. All validation
cases remain. Initialize Windows C++ tools through Developer PowerShell because
the vendored CommonJS action is incompatible with the repository's ESM scope.

Qualify the merged native ABI 5 implementation: all 28 library phases (including
host resources and networking) plus nine command phases, including commands run
inside active library callbacks. Require complete phase sets and moving-GC
witnesses for all six native workloads.

The Windows execution run exposed a compiler panic in loop-interrupt poll IR
construction: Inkwell requires a trailing NUL in copied memory buffers. Terminate
the poll IR and execute the four existing loop-placement regressions on every
qualification platform before running P1/P2 acceptance.

Record the expanded profile passing on Linux x86_64 and macOS ARM64, with
independently verified case outputs, complete phase sets, exact network payloads,
unit results, moving-GC witnesses and retained artifact hashes. Windows native
hosting and production component packaging remain separately unqualified.

Fix two further Windows failures exposed by the expanded run. Place writable
layout boundary markers alongside the writable descriptors, and enumerate both
read-only runtime atoms and writable generated atoms. Give shared ODR helper
functions named COFF COMDATs so MSVC can coalesce their definitions across
modules. Add regressions for emitted helper sections and actual linked-image
registration ranges; execute them in each platform qualification job.
