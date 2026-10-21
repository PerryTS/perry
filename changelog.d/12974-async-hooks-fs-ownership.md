### Tests

- Isolate the four remaining `async_hooks` filesystem fixtures with unique, captured temporary roots. Cleanup stays within each fixture's own root after resources are closed and hooks are disabled, preserving the original test bodies, assertions, and output.
- The existing baseline `FILEHANDLE` destroy-parity advisory remains; this change does not establish whole-module parity.
