// Perry compile-time intrinsics — issue #76 follow-up.
//
// `embedWasm("./path.wasm")` reads the file at compile time (relative to
// the importing source file) and bakes the bytes into the produced
// binary as a `Uint8Array` literal. The path argument MUST be a string
// literal — dynamic paths defeat the whole purpose and produce a
// compile-time error.
//
// This sidesteps TC39's in-flight Import Attributes proposal
// (`import bytes from "./x.wasm" with { type: "wasm" }`) which is not
// settled yet — we'll add the import-attributes form once the spec
// lands. See the maintainer's preference notes on issue #76.

/**
 * Compile-time embed of a `.wasm` (or any binary) file. Resolved
 * relative to the importing source file. The bytes are baked into the
 * final binary; no runtime file I/O.
 *
 * The argument MUST be a string literal known at compile time.
 *
 * The intrinsic is also exposed as an ambient global so it can be
 * invoked without an `import` (the underlying compile-time intercept
 * matches by function name regardless). Aliasing via
 * `import { embedWasm as foo } from "perry/build"` is **not** supported
 * — call by canonical name.
 */
export function embedWasm(path: string): Uint8Array;

declare global {
  /** See `embedWasm` documentation in `perry/build`. */
  function embedWasm(path: string): Uint8Array;
}
