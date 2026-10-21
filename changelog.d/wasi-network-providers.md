Add execution tests for the native component host's WASI UDP and name-resolution
providers. Exercise protocol and exact address/port grants, denied remote
endpoints, real loopback datagram exchange, separate DNS permission and fresh
context isolation. No external network service is required.

These tests cover the configured host providers directly; they do not establish
Perry guest UDP/DNS interoperability, blocking-I/O cancellation or additional
host-platform support. Execution qualification is pending.
