### Fixed

- **Destructured locals no longer inherit native receiver metadata from an
  unrelated same-named binding (#9487).** Perry retains native-instance
  classifications by binding name so large minified bundles can route calls to
  their native implementations. Simple declarations and parameters already
  shadowed stale classifications, but destructuring leaves did not. A prior
  `z = childProcess.spawnSync(...)` could therefore make a later, unrelated
  `const { install: z } = command` lower `z.call(...)` as a `child_process`
  native method rather than an ordinary JavaScript property call.

  Object shorthand and every recursively lowered keyed, array, nested, and
  rest leaf now apply the same native-instance tombstone as other fresh
  bindings. In Claude Code 2.1.112 this restores the install completion
  callback: the generated 64-hex `userID` is persisted and an invalid install
  exits 1 instead of falling out naturally with status 0. A HIR regression
  pins both keyed and shorthand binding hygiene, and the parity fixture checks
  failed/successful exits plus serialized `userID` validity against Node.
