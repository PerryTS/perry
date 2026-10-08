Typed fast tiers now resolve Native owners through the same byte-cell storage
resolver as byte access. Packed Uint32 columns and Float64 loop regions load
the out-of-line data word after their existing admission; Uint32 read-modify-write
uses the existing sealed-owner data slot when proven and otherwise resolves
storage before the read and again after the RHS. Existing owner roots and loop
proofs continue to govern hoisting and lifetime.

External side allocations now arm the precise safepoint through the existing
baseline-aware deferral helper. Map growth tests cover the nursery baseline,
actual safepoint collection and a stale-base negative control. Native buffer
birth tests cross the full 16 MiB allocation step and reject a planted collection.
Native typed-tier IR witnesses reject inline-only data derivation and admission;
Node parity covers Native owners, views, array annotation lies and detach.
