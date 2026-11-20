Reduce the per-call cost of Rust runtime GC handles. Scopes cache non-dropping
TLS metadata, root slots use 16 bytes, and idle pushes skip barrier dispatch.
Index handles retain bounds and kind checks across growth, moving collections,
and caught throws. Add fault-tested coverage for relocation, savepoint restore,
incremental marking, FFI indices, and TLS teardown. Measured user instruction
counts fall by 1.8–5.6% across JSON, promises, and the hoisted regex probes.

The merge train retains scoped Vec access on Android and HarmonyOS, where OS-backed TLS frees
metadata at thread teardown even for values without Drop. Native targets cache
the metadata as above; both paths retain the original four-slot initial allocation
floor. Teardown tolerates an already-destroyed Android hot-cache pool.
