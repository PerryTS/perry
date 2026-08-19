### Changed

- Native compilation now consistently uses `-O3`, including large modules and
  post-GC-rewrite functions. Perry no longer silently changes a module to `-Os`
  or `-O0`, or marks an oversized function `optnone`. Compile-time scalability
  is handled by codegen-unit partitioning and structured outlining, preserving
  the runtime optimization contract for every build.

- Successful internal Cargo, clang, Swift, and linker diagnostics are hidden at
  default verbosity so `perry compile` stays focused on TypeScript-developer
  actions. Pass `--verbose` for the full toolchain stream; failed tools still
  replay their captured output automatically.
