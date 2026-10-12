Recorded prototypes now live in each residual owner's ordinary property bag,
alongside its own properties. Ordinary objects retain their shape authority.
Deletes OBJECT_PROTOTYPES, its nonempty latch and residual-owner bit, the
per-cell GC visits, relocation rekey and death/thread-exit prunes. Array index
guards retain their existing per-array custom-prototype flag; neither deferred
array table is changed. Native registry handles reuse their existing property
holder; internal native payload cells remain outside descriptor routing. Moving owner/prototype tests,
a fresh-owner gap control, and a mutation that clears the owned prototype slot
cover the storage contract.
