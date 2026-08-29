### Added

- **Static Zod object schemas can now take a native `z.compile()` path.** Perry
  recognizes supported `z.object({...})` declarations and lowers string,
  finite-number, boolean, integer, and literal numeric-bound checks directly
  to HIR. Successful parses no longer require Zod's `new Function` fast path;
  failed checks delegate to the original Zod parser so its issue construction
  remains authoritative. Unsupported schemas keep the regular Zod call.
