### Fixed

Local objects and function parameters named `crypto`, `fs`, `path`, `os`, or
`net` now use their own properties and methods instead of being mistaken for
Node builtin namespaces. This restores userland helpers such as pg's SCRAM
crypto module while preserving intrinsic lowering for real builtin imports.
