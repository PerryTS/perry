Add a measurement-only runtime feature, `perry-runtime/fixed-hash-seeds` (off by
default, never shipped): the startup-path `ahash` maps (`fast_hash::AHashState`:
native callable exports, static name maps, the JSON parser, timer ref states and
the unwinder's site cache) seed from fixed constants instead of
`RandomState::new()`, whose seed mixes in static and stack addresses. Exact-Ir
diffs then stop moving with the linker's layout. The default build keeps
`ahash::RandomState::new()`: an unpatched and a patched default build have
identical total Ir on hello, commander --help, fastify and Richards.

`scripts/a1/build-arm.sh` builds an arm with the feature and
`scripts/a1/irdiff.sh` refuses arms built without it.
