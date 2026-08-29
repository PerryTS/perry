### Added

- **Static Zod object schemas can now take a native `z.compile()` path.** Perry
  recognizes supported `z.object({...})` declarations and lowers string,
  finite-number, boolean, integer, and literal numeric-bound checks directly
  to HIR. Perry supplies only the generated parser through Zod's
  `compileFromParser()` integration API; Zod continues to own schema cloning,
  parse-context bypasses, public methods, validation, and runtime fallbacks.
  Successful parses no longer require `new Function`, while failed checks and
  unsupported schemas remain authoritative in Zod.
- `perry.compilePackages` can now resolve a package's TypeScript source tree
  even when its published JavaScript export target is absent, enabling
  reproducible source-only package pins for native compilation.
