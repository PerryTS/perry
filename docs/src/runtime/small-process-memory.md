# Small-process memory profile

On Linux x64 and ARM64 builds using mimalloc, launch a native Perry executable
with `PERRY_MEMORY_PROFILE=small` to prefer lower resident memory for short
programs. This defaults mimalloc's `allow_thp` option to zero before startup
allocations. The normal profile retains mimalloc's existing policy.

```sh
PERRY_MEMORY_PROFILE=small ./app
```

This profile applies to programs that retain the managed runtime. Automatically
specialized [tiny programs](tiny-programs.md) already omit Perry's allocator and
GC, so they do not need this setting.

An explicit `MIMALLOC_ALLOW_THP=1` overrides the profile; `=0` also works without
the profile. Mimalloc still parses these settings itself. Set the environment
before launching the executable. Changing it from JavaScript is too late to
configure the initial heap. Other profile names and unsupported platforms keep
the existing allocator policy.

The policy is a tradeoff. Earlier Linux experiments with the equivalent
allocator setting roughly halved tiny-program RSS, but startup gains varied
between CPUs and allocation-heavy workloads sometimes slowed down. Measure
your workload; the profile does not promise a startup or throughput improvement.

On Linux, mimalloc uses `PR_SET_THP_DISABLE` for this option. That affects the
entire process, including memory managed by other allocators. Embedders that
share mimalloc with a host must configure it before the host's first allocation;
loading Perry later cannot retroactively change the initial heap policy. Setting
`MIMALLOC_ALLOW_THP=0` at host process launch makes that intent explicit.

## Realm population

Native compilation enables two-phase realm population by default. A static read
of a core global initializes the core builtins; observing the global object or
a builtin outside the core completes the realm. If the first access requires
the complete realm, the runtime populates it in one pass. Both entries refer to
the same global object, and each retained copy is registered as a GC root.

The deferred Web families are `DOMException`, `Event`, `CustomEvent`,
`MessageEvent`, `AbortController`, and `AbortSignal`. Their constructors and
prototypes use the same birth routines and property order in either phase.
Runtime-created objects materialize their intrinsic family before exposing it.
`EventTarget` stays in the core because `Performance` inherits from it;
`Storage` and its global accessors also stay in the core. `Response`, `WebSocket`,
`URL`, `URLSearchParams`, and `SubtleCrypto` stay in phase A while their prototype
surfaces and property order need Node parity work.

Population rows derive their phase and sealing eligibility from the canonical
lists at compile time. Completing a realm reads those flags without matching
each constructor name again or calling sealing helpers for ineligible families.

To compile with one-pass population throughout, set the compiler environment:

```sh
PERRY_REALM_TWO_PHASE=0 perry compile app.ts -o app
```

`off` and `false` also disable two-phase population. This is a compile-time
setting and participates in the compilation cache key; setting it when launching
an already compiled executable does not select another population strategy.
The one-pass executable omits the two-phase runtime path.

On Linux, the two-phase population occupies a separate tail section after the
program's generated text. This keeps that code out of the common runtime text
layout. It adds executable bytes, so memory gains depend on which builtins the
program observes. Compare warm peak RSS and startup instructions on the same
workload when assessing the tradeoff.
