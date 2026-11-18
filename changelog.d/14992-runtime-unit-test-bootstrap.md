### Testing

- `perry-runtime` unit tests pass on macOS again: snapshot-capture tests that read `/proc/self/maps` run on Linux only, and the TLS teardown witness runs in its own subprocess so the macOS SIGTRAP termination is accepted without depending on the standard library's panic text.
