**fix(release): restore static Linux musl packages from a glibc cross-build host (#9382)**

The x86_64 and AArch64 musl release legs are enabled again. They now run Cargo
and bindgen on a glibc host, where the `rusqlite/session` build script can
`dlopen` libclang, while linking Alpine's musl-built LLVM 22 and static system
libraries into the target artifacts.

The release build rejects any resulting compiler with an interpreter or
dynamic dependency before it can be packaged. The two musl npm packages are
restored to staging, publish ordering, and the wrapper's optional dependencies.
