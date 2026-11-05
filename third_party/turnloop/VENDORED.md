# Qualified Turnloop provider sources

Perry patches `turnloop` alpha.10 and `turnloop-http` alpha.9 to the exact Rust sources
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
The patched packages are pinned to turnloop alpha.10 and turnloop-http
alpha.9; the direct `turnloop-io` dependency remains pinned to alpha.9. Other dependencies retain their existing lockfile versions.

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

The main-branch landing reconciles the published `turnloop` alpha.10 source
(commit e31526b5399d00e94b1452ffe8a8e0e76e4824ad) with these provider fixes.
The alpha.10 native socket-adoption and socket-control APIs are retained for
Perry’s existing synchronous embedding boundary; the qualified executor UDP,
bound-connect, stdio, receive-shutdown and HTTP casing repairs remain. Unix and
IOCP keepalive parameter setters use the alpha.10 validation helpers without
changing whether keepalive is enabled. This is still a patched source snapshot.
Historical receipts qualify their recorded source only; the combined landing
requires fresh validation. The original registry/provenance hashes are retained
for comparison, with the integration source hashes recorded separately.

The IOCP unit-test module is extracted to `backend/iocp/tests.rs` to meet
Perry’s 2,000-line source cap; its module path and visibility are unchanged.

Patched by Perry after the qualification above (HTTP/2): PUSH_PROMISE received and sent,
role-aware ENABLE_PUSH validation (a client no longer sends ENABLE_PUSH = 0), a
HEADER_TABLE_SIZE cap that follows the advertised value, GOAWAY(INTERNAL_ERROR) for a PING
acknowledgement with no PING outstanding, and SETTINGS that carry only the options the host
names. Touched: `turnloop-http/src/http2.rs`, `src/hpack.rs`, `tests/http2_settings.rs`,
`tests/codecs.rs`; their hashes in `source-manifest.json` are the patched ones, and
`cargo test -p turnloop-http` passes on this tree in a scratch workspace.
