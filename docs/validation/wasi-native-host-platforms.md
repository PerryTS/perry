# Native WASI host platform qualification

Run the `CI` workflow (`test.yml`) with `suite=wasi-hosts` on the revision to
qualify. This builds and executes the native hosts on Linux x86_64, macOS ARM64
and Windows x86_64. Each platform uploads a separate `wasi-hosts-*` artifact,
including `qualification.json`, per-case results, unit-test output and logs.

A passing platform requires both native host crate unit suites, the complete P1
Node parity suite (currently 56 cases plus a moving-GC execution), and the native
P2 component interoperability suite. Node must match `.node-version` exactly.
Expected process exits are checked independently of output comparison. Both P1
and P2 moving runs must show copied objects, evacuation verification and
quarantined from-space. The receipt identifies the source revision and hashes
the exact compiler, tools and host/runtime archives used.

The initial suite is manual and does not establish platform support until its
execution evidence passes. It does not qualify standalone WASI output, command
component hosting, unmerged host-resource or cleanup changes, guest networking,
HTTP, cancellation or Preview 3. Standalone output has its own release-artifact
qualification. Existing required checks and dependency policies still apply.

For local execution, build the same artifacts and GC-effect sidecars as the
workflow, capture both host unit suites in `OUTPUT/native-host-units.log`, then
run `scripts/wasi_host_qualification.py --help` for the required paths. A clean
tracked source tree is required, and receipts must be paired with matching build
logs rather than inferred from an unrelated prebuilt compiler.
