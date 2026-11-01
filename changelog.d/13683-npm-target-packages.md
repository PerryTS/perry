### Changed

- Cross-target libraries ship on npm as one host-independent package per target, `@perryts/perry-target-<triple>` (android aarch64/x86_64, iOS device/simulator, wasm32-wasip2/wasip1), instead of inside the host packages. Windows npm users get `--target android` from `@perryts/perry-target-aarch64-linux-android`; the Windows packages no longer carry the android libraries (they made the upload too large for npm).
- `perry compile --target <t>` finds a target package installed beside perry (global, project or pnpm) or in `~/.perry/targets`, uses it only when its `target.json` names perry's exact version and source revision, and otherwise prints the exact install command.

### Added

- `perry target add | list | remove`: install a cross target's libraries from the GitHub release without npm, verified against a manifest signed with the CLI-update key (`perry updater sign-target-manifest`, its own signing domain). Release and nightly runs attach the signed target tarballs.
