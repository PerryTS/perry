# Qualified Turnloop provider sources

Perry patches `turnloop` and `turnloop-http` alpha.9 to the exact Rust sources
used for its Linux provider, native async and concurrent HTTP qualification.
Root Cargo patches select these copies for the complete graph. Builds require
no external Cargo configuration or build-host source directories.

The normalized manifests and package files come from checksum-verified crates.io
alpha.9 packages. Their `src/` trees and READMEs come from the qualified
`turnloop-http-case-v3-src` snapshot, used by the
[concurrent HTTP qualification](../../docs/validation/wasi-concurrent-http-20261026.json).
The original manifests match the qualified workspace manifests exactly.
MIT licenses are included in both packages.
Package-local lockfiles are omitted because Perry resolves the graph in its
workspace lock. One trailing space in the bundled HPACK corpus copyright line
is removed for Git whitespace checks; the license text is otherwise unchanged.

`source-manifest.json` records the registry checksums, the qualified source
archive checksum and every selected Rust source hash. `turnloop-io` alpha.9
remains a registry dependency: all its Rust sources are byte-identical to the
qualified copy, and its Turnloop dependency resolves through the root patch.
The two patched package versions and the direct `turnloop-io` dependency are
pinned to alpha.9. Other dependencies retain their existing lockfile versions.

Cargo patches belong to the root workspace. The standalone Rust acceptance
fixture declares the same two patches, and its harness rewrites their paths
when copying the fixture. Rust applications embedding Perry's source crates
from another workspace must likewise select these patches in their own root
manifest. Hosts consuming emitted WASI components do not need Perry's Cargo
patches.

The source repairs cover overlapping stdio ownership, bound TCP connections,
independent socket controls and receive shutdown, datagram packet/address
preservation, and HTTP header/trailer casing. Earlier provenance and upstream
repair links are recorded in
[the provider record](../../crates/perry-wasi/PROVENANCE.md).

Published alpha.10 was checked on 2026-10-26. It still lacks `AsyncDatagram`,
`ExecutorHandle::datagram`, `ExecutorHandle::connect_bound`,
`AsyncIo::shutdown_receive`, and `Header::new_preserving_case`, so adopting
that release alone cannot replace these copies. Do not label this snapshot as
unmodified upstream alpha.9 or alpha.10.

Retain the upstream module layout to keep source comparisons reproducible.
Replace these patches with released dependencies only after checking the used
APIs and behavior and repeating provider/component qualification. Vendoring
does not qualify other platforms or final release packages.
