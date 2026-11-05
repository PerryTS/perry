# Synchronous WASI component hosting

`perry-component-host` implements the stable WASI Preview 2 command imports on
Wasmtime's synchronous component API. The adapter lives in `src/sync_wasi`.
Its WIT is the unchanged 0.2.12 snapshot described by `wit/provenance.json`;
`add_to_linker` installs all 27 interfaces used by the previous host. The
optional archive is selected only for programs importing `perry/component`.

## Store state and authority

Each `WasiState` owns its capability options, Wasmtime resource table, clock
origin and preopened directories. Defaults expose no environment, inherited
stdio, preopens or networking. Filesystem operations use capability-relative
directories and enforce the existing read-only grants. Socket creation,
binding, connection, datagram destinations and received peers enforce the
existing protocol and exact address/port grants. DNS has its separate grant.

Files and inherited stdio use blocking standard I/O. Clocks, randomness and
socket options use their native synchronous APIs. The adapter has no Tokio
dependency or asynchronous Wasmtime feature.

## Network completion ownership

Native networking and timer subscriptions use `perry-component-io`, through
the versioned C operation table in `perry-abi::component_io`. Perry's link
installer supplies the runtime's immutable table before creating stores.
That table uses the agent's existing turnloop driver. The standalone Rust
embedding supplies one lazy driver per embedding thread, shared by all its
stores; creating a store never creates a driver.

A guest poll turns that driver and extracts only its exact operation IDs.
Other completions remain in the existing FIFO staging queue. Guest waits
never dispatch JavaScript callbacks or promise reactions. Ordinary event-pump
dispatch resumes after the guest returns. Closing resources selects the exact
handle's close acknowledgement.

An operation owns its native read buffer or transferred write buffer until
terminal completion. Dropping an unfinished operation cancels it and drains
its acknowledgement before freeing storage. Operations retain their socket,
and the final socket owner drains close acknowledgement. A failed driver
cannot prove buffer quiescence: that exceptional cleanup path retains the
native allocation rather than freeing storage the OS might still reference.

Socket/stream phases, queued bytes and pending operations represent actual
protocol state and ownership. Socket settings are read from the kernel.
Changing a quiescent UDP association transfers the native socket out of the
driver and adopts it again; no borrowed descriptor is mutated behind the
driver. Bound TCP connect preserves the guest's chosen local endpoint. Unbound TCP
leaves port allocation to Unix connect; Windows binds during adoption for
ConnectEx. Adoption selects the API from the socket's actual native endpoint.

## Thread and allocator boundaries

A `WasiState` is confined to its embedding thread. Native resource entries
also verify thread identity because Wasmtime requires mapped resource types
to implement `Send`. No guest operation migrates between drivers.

Pointers and completion buffers cross the native archive boundary only through
the C table. The allocating module supplies the matching destruction function.
The runtime and optional host do not exchange Rust-owned containers or trait
objects across that boundary.

## Validation

The host unit tests cover capability isolation, all stable provider groups,
file confinement and mutation, stream ownership, TCP/UDP traffic and polling,
DNS, command exits, component resource lifetimes and interruption. The runtime
test checks that a guest wait preserves unrelated completions and other
component operations. `test-wasi/components/network` additionally schedules a
JavaScript timer during a guest wait and checks deferred, exactly-once delivery.

Run the three `scripts/component_*_acceptance.py` harnesses with matching
compiler/runtime archives to exercise native hosts, command guests and Perry
library guests, including the independent Rust host and moving-GC witnesses.
The strict Tokio inventory and cargo-deny ban remain unmodified.
