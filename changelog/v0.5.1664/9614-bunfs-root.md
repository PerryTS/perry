### Bun compatibility

- **`perry compile --bunfs-root <DIR>` now compiles source extracted from a
  Bun standalone executable without a host `/$bunfs` mount.** Static imports,
  re-exports, literal dynamic imports, and literal `require()` calls retain
  their Bun virtual paths while resolving against the extracted directory.
  Canonical real paths keep a module imported through both spellings to one
  identity, including modules below an extracted `node_modules` directory.

  Literal mapped files are embedded under their original `/$bunfs/root/...`
  names, so `node:fs` and `Bun.file()` reads keep working after the source tree
  moves. Missing module mappings produce a focused diagnostic, and mapped
  paths cannot traverse or follow symlinks outside the configured root.
