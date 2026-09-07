### Fixed

- Linux npm installs now find their bundled runtime, standard-library, and
  GTK4 archives when the host target is spelled explicitly with `--target
  linux`. Cross-architecture and cross-libc targets remain isolated to their
  target-specific archive directories.
