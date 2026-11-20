Fix the GC call-effects census for native C exports by deriving archive-member ownership from the runtime sources and analyzing their bodies with the existing call graph. This includes `js_back_hot_text` without assigning it an unchecked effect.

Keep the three IC read entries GC leaves by removing general, potentially materializing array resolution from their shared array-holder proof. The GC header and array shape flags admit only a live ordinary array; lazy JSON arrays and growth forwarding stubs decline to the existing collecting continuation. The proof reads the intrinsic prototype's property bag through the same resolved-array load used by the general bag helper, after the intrinsic cache heals forwarding. The emitted read sequence gains no guards, caches, or per-site helpers.

The function-holder proof follows the same rule: read the fixed intrinsic dictionary ShapeId seeded at agent creation, without initializing shape metadata. This preserves fixed-layout closure and bound-function births. No new cache is introduced.

Add lazy JSON IC coverage under moving collection, with controls for own-key shadowing and materialization. Preserve the call-effects and root-dominance checks.

Refresh the Linux table from the merged runtime archives. Conservatively refresh the macOS and Windows startup entries to `Reenters` after the audited graph reaches intrinsic-shape slab destruction through state initialization.

Refresh the two new fixed/small closure-birth WASI ABI rows from the runtime signature generator: full-width unsigned words use `i64`, not the unsupported `i64u` token. This restores the shadow-root audit without changing native code generation.

Use the existing runtime image-handle boxing path on wasm32, whose object writer cannot encode a tagged 64-bit pointer relocation in static initializers. Preserve native ELF output and cover both WASI and unknown wasm32 targets in the existing image-handle regression.

Teach the root-dominance checker to parse `volatile` separately from a store’s value type. Previously it ignored volatile root stores, missing two of the 40 planted shadow-root violations. Add hazardous and correctly rooted volatile-store controls; retain every audit floor and budget.

Apply the same volatile-access parsing to local `i1` guard cells, so the existing flag-correlation proof sees the array-region base refresh. Exercise all six safe/broken-refresh cases and fail-closed budget controls with volatile accesses as well.
