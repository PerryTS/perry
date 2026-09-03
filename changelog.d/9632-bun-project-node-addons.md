### Bun compatibility

- **Root/project Node-API addons loaded through `import.meta.require` now ship
  and run after a Bun extraction tree is removed.** Declare each file by its
  exact project-relative path, for example
  `"perry": { "nativeAddonPaths": ["native/addon.node"] }`. Perry follows
  immutable aliases and simple path constants, including
  `new URL("./native/addon.node", import.meta.url).pathname`, and maps relative,
  absolute, and `/$bunfs/root/` spellings to the same authenticated sidecar
  entry. Dynamic or otherwise unprovable paths fail compilation with guidance
  instead of producing a binary that depends on the build machine's source
  tree.
