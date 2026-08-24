# Vendored Windows Reactor snapshot

This directory contains the Windows Reactor crate and the matching unreleased
`windows-*` support sources needed by Perry's WinUI 3 backend. The source was
copied from Microsoft `windows-rs` commit
`65066a7109c214f317ed66261cfb7518160b8aaf` (the merge commit for
`microsoft/windows-rs#4479`).

The manifests only differ where needed to replace the upstream workspace
dependencies with local paths and published Rust ecosystem dependencies. The
Reactor build script is also adapted to stage the framework-dependent
bootstrap DLL, its import library, and the XAML resource PRI in Perry's Cargo
target directory. Perry also adds an application-exit callback hook so its
existing lifecycle ABI can run termination handlers when the WinUI window
closes.

The upstream MIT and Apache-2.0 license files are preserved in every vendored
crate directory.
