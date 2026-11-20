Fix a namespace binding losing its kind through an `import { X } from "m"; export { X }` re-export (#13091).

- The scan that classifies an imported local as a module namespace followed `export { X } from "m"` and `export *` but not the import-plus-`export { X }` form. With that hop between a consumer and an `export * as`/`import * as` origin, the local was never registered as a namespace: `X.member()` lowered to a closure singleton over `__perry_wrap_<origin>__X$info`, a symbol a namespace never has, and the link failed with `undefined reference`. #11275 covered the other two hops.

Validation: `test-files/test_parity_namespace_reexport_named_hop.ts` fails to link before the change and prints `T:x` / `function` after, matching Node.
