# Component provider I/O

This crate is the native I/O boundary for the Turnloop provider migration.
It is not yet connected to `perry-component-host`, and does not itself implement
WASI interfaces. The existing host still depends on `wasmtime-wasi`; the
workspace's no-Tokio gate remains failing until those providers are replaced.

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

Factories must not block the I/O thread, capture the `Driver`, or detach child
tasks. Retain each submitted request until completion or intentional cancellation.
Blocking filesystem work must use Turnloop's bounded worker pool; cancellation
of an already running blocking system call is cooperative and cannot promise
to undo completed filesystem effects. That provider integration is still pending.
Rust panic recovery applies only to unwind builds; Perry's shipping profiles
abort on a Rust panic.

The tests exercise actual loopback TCP, write half-close and reply reading,
pending-read cancellation and peer EOF, timer progress beside pending work,
capacity/re-admission, cancellation before a factory runs, non-Send future
ownership, nested submissions, independent contexts and joined shutdown. Every
cross-thread test has a watchdog. Run with:

```sh
cargo test --locked -p perry-component-io -- --test-threads=1
cargo test --locked --profile prod -Z panic-abort-tests -p perry-component-io --lib -- --test-threads=1
cargo clippy --locked -p perry-component-io --all-targets -- -D warnings
```

The next integration must retain the existing provider contracts: isolated
arguments/environment/preopens, traversal and symlink confinement, descriptor
and stream lifetimes, explicit stdio, exact IP/port grants, TCP bind-before-connect,
UDP peer restrictions, DNS opt-in and typed WASI errors. Passing this crate's
tests is not evidence that those contracts, HTTP, or WASI 0.3 have been ported.
