Construct JSON and Reflect with their complete canonical property layouts,
reusing the bulk namespace builder introduced for Math. This avoids repeated
shape transitions and overflow-property installation during bootstrap while
preserving method metadata, descriptors, property order, feature gating and
ordinary mutation behavior. Each namespace still receives fresh function
objects; no heap-pointer caches or additional GC roots are introduced.

Add coverage for inline storage, key sharing, independent functions, detached
calls and optional arguments, descriptor mutation, inheritance, freezing,
moving GC and isolation between threads. Validate against Node 26.5.1 and six
larger programs, including forced moving-GC runs.

On the measured Linux x86-64 build, materialized JSON/Reflect startup probes
execute approximately 0.2%/0.6% fewer instructions, or 0.8% with both present.
Default empty and hello-world executables remain byte-identical. These are
instruction-count improvements, not a general application-throughput claim.
