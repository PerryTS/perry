Fix quadratic Buffer/Uint8Array subarray creation by allocating header-only
views over shared backing bytes. Trace view owners and cached ArrayBuffer
identities through the GC, and remove dead views without scanning unrelated
registries. Preserve nested/empty view offsets, shared writes and overlapping
copies; keep Uint8Array and ArrayBuffer slice results independent.
Resolve shared bytes in JSON/console formatting, array conversion, crypto,
compression, networking and native FFI consumers.

Add allocation, aliasing, detach, moving-GC lifetime/reclamation and Node parity
regressions. Measurements and the unchanged issue workload are recorded in
`benchmarks/issue-10056/`.
