# Component provider I/O

This crate is the native I/O boundary used by `perry-component-host` through
`perry-wasi`. It does not itself implement WASI interfaces. The workspace
no-Tokio gate passes with the selected provider. The required Turnloop repairs
are currently qualified with an external validation patch; see
[`perry-wasi` provenance](../perry-wasi/PROVENANCE.md) for release prerequisites.

`Driver` owns a Turnloop executor on one native thread. A component context owns
the driver. Its resources hold weak `Submitter` endpoints, which cannot keep the
driver alive after context teardown. Submitted factories construct local,
potentially non-Send futures on the I/O thread; only owned Send native inputs and
results cross the channel. Wasmtime stores, guest execution, canonical lifting,
Perry heap pointers and JavaScript callbacks stay on the calling Perry agent.

The admission limit counts queued and running tasks. Each returned `Request`
cancels its task on drop. Explicit cancellation followed by awaiting the request
observes disposal of the local future and its native resource owners. Native
close completions may still need a loop turn. Dropping the driver stops new
admissions, drops pending futures and joins the I/O owner. A submitter cannot
extend that lifetime, even if it temporarily upgrades its weak reference during
a concurrent submission. Requests do not retain the driver.

`Submitter::register` creates a persistent native value on the I/O thread and
returns a Send `Resource<T>`, even when `T` is not Send. `Resource::call` constructs
a local future with access to that same value. `LocalResource::borrow_mut` is for
one synchronous operation or poll; never hold its guard across an await. This
allows a pending socket read and a write to share one native connection without
reopening or duplicating it for each request. Only owned native values belong in
this table; it must never hold a Wasmtime Store or Perry guest heap reference.

`Resource::derive` creates another native resource on the same driver, such as
an accepted connection from a listener. It reserves both task and resource
capacity before invoking the factory. Rejection therefore cannot consume an
incoming connection. The factory receives a local parent owner for the operation;
the resulting resource has its own lifetime and can outlive the listener.

The resource limit is independent of the task limit and includes pending
registrations and values retained by in-flight operations. Dropping the last
remote handle queues a release without requiring a task permit. Pending calls
retain their local value until completion or cancellation. The driver drains
release commands before parking, including batches sharing a coalesced wake.
Context teardown releases every table entry and pending call even when remote
handles survive. Cancelling a call does not destroy a resource with other owners:
providers must manage any operation state retained inside it explicitly.

Factories must not block the I/O thread, capture the `Driver`, or detach child
tasks. Retain each submitted request until completion or intentional cancellation.
Blocking filesystem work must use Turnloop's bounded worker pool; cancellation
of an already running blocking system call is cooperative and cannot promise
to undo completed filesystem effects. The filesystem provider uses this pool.
Rust panic recovery applies only to unwind builds; Perry's shipping profiles
abort on a Rust panic.

The tests exercise actual loopback TCP, write half-close and reply reading,
pending-read cancellation and peer EOF, timer progress beside pending work,
capacity/re-admission, cancellation before a factory runs, non-Send future
ownership, nested submissions, independent contexts and joined shutdown. Every
cross-thread test has a watchdog. Persistent-resource tests additionally cover
owner-thread construction/access/destruction, separate admission budgets,
failed/undelivered registrations, cancellation and capacity recovery, retained
owners, context teardown, coalesced release batches, exact concurrent TCP bytes
and peer EOF after cancelling the last pending native owner. Run with:

```sh
cargo test --locked -p perry-component-io -- --test-threads=1
cargo test --locked --profile prod -Z panic-abort-tests -p perry-component-io --lib -- --test-threads=1
cargo clippy --locked -p perry-component-io --all-targets -- -D warnings
```

The provider integration retains these contracts: isolated
arguments/environment/preopens, traversal and symlink confinement, descriptor
and stream lifetimes, explicit stdio, exact IP/port grants, TCP bind-before-connect,
UDP peer restrictions, DNS opt-in and typed WASI errors. Provider tests cover
those contracts separately. HTTP, WASI 0.3 and complete platform qualification
remain outstanding.
