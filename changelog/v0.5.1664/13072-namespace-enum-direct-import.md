Fix a namespace import replacing a separately imported enum that shares its declaration name (#13030).

- `import * as schema` and named `export * as` re-exports registered each member enum under its bare export name, so `import { Kind }` from another module read `schema.Kind`'s values. Namespace enum members are runtime property reads and no longer register a lexical enum binding, matching the existing class handling.

Validation: `test-parity/node-suite/module/imports/namespace-enum-direct-import.ts` (expected output in `test-parity/expected/`, since Node cannot strip `enum`) reads the direct enum as `2` before the fix and `1` after it.
