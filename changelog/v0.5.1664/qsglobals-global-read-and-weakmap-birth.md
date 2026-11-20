Route global constructor static-value reads through ordinary shape-validated property read sites, including the constructor read from `globalThis`. Reassignment and lexical shadowing retain their normal JavaScript semantics.

Create WeakMap and WeakSet objects with their final prototype and private brand in one birth shape, removing the empty literal allocation and subsequent prototype and brand transitions. Derive the exact weak collection brand from the compact shape header instead of searching its auxiliary brand list; shape identity still includes the complete brand list.

Add global reassignment/accessor/shadowing coverage, weak collection GC and ephemeron coverage, and codegen/header assertions. No version fields change.
