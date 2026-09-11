# Small-process memory profile

On Linux x64 and ARM64 builds using mimalloc, launch a native Perry executable
with `PERRY_MEMORY_PROFILE=small` to prefer lower resident memory for short
programs. This defaults mimalloc's `allow_thp` option to zero before startup
allocations. The normal profile retains mimalloc's existing policy.

```sh
PERRY_MEMORY_PROFILE=small ./hello
```

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
