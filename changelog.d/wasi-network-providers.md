Add execution tests for the native component host's WASI UDP and name-resolution
providers. Exercise protocol and exact address/port grants, denied remote
endpoints, real loopback datagram exchange, separate DNS permission and fresh
context isolation. No external network service is required.

These tests cover the configured host providers directly; they do not establish
Perry guest UDP/DNS interoperability, blocking-I/O cancellation or additional
host-platform support. All 22 component-host unit tests passed on Linux x86_64 at
`bd0d064baa9217a33f5215c819ae890845e7944b`, including these three new tests.
The source revision and evidence hashes are recorded in
`docs/validation/wasi-network-providers-20261021.json`.
