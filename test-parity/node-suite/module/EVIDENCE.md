# `node:module` evidence matrix

This file makes the focused audit reproducible at the per-entry level. Source
paths refer to the pinned upstream trees and commits listed in `README.md`.
“match” means stdout and exit code exactly match Node 26.5.0; “ok-diff” means
the runtime exits successfully with intentionally different stdout; `exit-1`
means the API/contract is unavailable or rejects the probe. Perry statuses use
the suite runner buckets.

## Reproduction

```sh
NODE_BIN="$HOME/.nvm/versions/node/v26.5.0/bin/node" \
  python3 scripts/node_suite_run.py "$PWD/target/release/perry" "$PWD" module
# Run each staged-added .ts entry twice with Node, then once with:
deno run --allow-all <entry>
bun run <entry>
```

Two complete Node runs, including file headers, were byte-identical at
`85ffb7464d6c8e860fb2741cc635c2d59711636d1dfafb5a742a8355c95ab1a2`. Two focused
runner reports were byte-identical at
`1a8fee9fa4a5f8267e262d3623a6830a4226858bbb22ea315db724abaafb6eb4`; two per-file
Perry classifications were byte-identical at
`3eddc8e08d9e49fffe8633061efd8bb7e566e3f41e664a2e3e291d5bcda30ef6`.

## Added-entry matrix

| Entry                                  | Primary Node contract                                                                      | Perry          | Deno 2.9.2 | Bun 1.2.18 |
| -------------------------------------- | ------------------------------------------------------------------------------------------ | -------------- | ---------- | ---------- |
| `commonjs/constructor-prototype.ts`    | `lib/internal/modules/cjs/loader.js`; `test-module-wrap.js` / `test-module-children.js`    | `diff`         | `ok-diff`  | `ok-diff`  |
| `commonjs/prototype-descriptors.ts`    | `lib/internal/modules/cjs/loader.js`; `test-module-wrap.js` / `test-module-children.js`    | `diff`         | `ok-diff`  | `ok-diff`  |
| `commonjs/wrap-wrapper.ts`             | `lib/internal/modules/cjs/loader.js`; `test-module-wrap.js` / `test-module-children.js`    | `diff`         | `match`    | `ok-diff`  |
| `exports/builtin-modules.ts`           | `lib/module.js`; `test-module-builtin.js`; `test-module-isBuiltin.js`                      | `diff`         | `ok-diff`  | `ok-diff`  |
| `exports/constants.ts`                 | `lib/module.js`; `test-module-builtin.js`; `test-module-isBuiltin.js`                      | `diff`         | `exit-1`   | `ok-diff`  |
| `exports/default-static-identity.ts`   | `lib/module.js`; `test-module-builtin.js`; `test-module-isBuiltin.js`                      | `diff`         | `ok-diff`  | `ok-diff`  |
| `exports/descriptors.ts`               | `lib/module.js`; `test-module-builtin.js`; `test-module-isBuiltin.js`                      | `diff`         | `exit-1`   | `match`    |
| `exports/public-surface.ts`            | `lib/module.js`; `test-module-builtin.js`; `test-module-isBuiltin.js`                      | `diff`         | `ok-diff`  | `ok-diff`  |
| `helpers/compile-cache-disabled.ts`    | compile-cache exports in `lib/module.js`; `lib/internal/modules/helpers.js`                | `pass`         | `exit-1`   | `exit-1`   |
| `helpers/strip-options-validation.ts`  | `test-module-strip-types.js`; `lib/internal/modules/typescript.js`                         | `pass`         | `ok-diff`  | `exit-1`   |
| `helpers/strip-syntax.ts`              | `test-module-strip-types.js`; `lib/internal/modules/typescript.js`                         | `diff`         | `match`    | `exit-1`   |
| `imports/cjs-dynamic-identity.ts`      | `lib/internal/modules/esm/translators.js`; package/JSON cache tests under `test/es-module` | `diff`         | `match`    | `ok-diff`  |
| `imports/cjs-namespace.ts`             | `lib/internal/modules/esm/translators.js`; package/JSON cache tests under `test/es-module` | `diff`         | `match`    | `ok-diff`  |
| `imports/json-attribute.ts`            | `lib/internal/modules/esm/translators.js`; package/JSON cache tests under `test/es-module` | `pass`         | `match`    | `match`    |
| `imports/json-require-identity.ts`     | `lib/internal/modules/esm/translators.js`; package/JSON cache tests under `test/es-module` | `diff`         | `ok-diff`  | `ok-diff`  |
| `imports/package-conditions.ts`        | `lib/internal/modules/esm/translators.js`; package/JSON cache tests under `test/es-module` | `compile_fail` | `match`    | `match`    |
| `loader/register-hooks-order.ts`       | `lib/internal/modules/customization_hooks.js`; Deno `module_register_hooks*` specs         | `diff`         | `match`    | `exit-1`   |
| `methods/find-package-json.ts`         | `lib/internal/modules/package_json_reader.js`; `findPackageJSON` in `lib/module.js`        | `diff`         | `ok-diff`  | `exit-1`   |
| `methods/function-shapes.ts`           | `lib/module.js`; `test-module-isBuiltin.js`                                                | `diff`         | `exit-1`   | `exit-1`   |
| `methods/is-builtin-validation.ts`     | `lib/module.js`; `test-module-isBuiltin.js`                                                | `diff`         | `match`    | `match`    |
| `methods/sync-builtin-exports.ts`      | `lib/internal/modules/esm/translators.js`; builtin live-binding tests                      | `diff`         | `ok-diff`  | `ok-diff`  |
| `require/cache-identity-deletion.ts`   | `lib/internal/modules/cjs/loader.js`; `test-module-create-require.js`                      | `diff`         | `match`    | `match`    |
| `require/create-require-overloads.ts`  | `lib/internal/modules/cjs/loader.js`; `test-module-create-require.js`                      | `diff`         | `match`    | `match`    |
| `require/cycles.ts`                    | `lib/internal/modules/cjs/loader.js`; `test-module-cache.js` / `test-module-children.js`   | `diff`         | `match`    | `match`    |
| `require/error-fields.ts`              | `lib/internal/modules/cjs/loader.js`; `test-module-create-require.js`                      | `compile_fail` | `match`    | `ok-diff`  |
| `require/exports-alias.ts`             | `lib/internal/modules/cjs/loader.js`; `test-module-create-require.js`                      | `diff`         | `match`    | `match`    |
| `require/extensions.ts`                | `lib/internal/modules/cjs/loader.js`; `test-module-create-require.js`                      | `diff`         | `exit-1`   | `ok-diff`  |
| `require/function-descriptors.ts`      | `lib/internal/modules/cjs/loader.js`; `test-module-create-require.js`                      | `diff`         | `ok-diff`  | `ok-diff`  |
| `require/json-cache.ts`                | `lib/internal/modules/cjs/loader.js`; `test-module-create-require.js`                      | `diff`         | `match`    | `match`    |
| `require/module-load.ts`               | `lib/internal/modules/cjs/loader.js`; `test-module-cache.js` / `test-module-children.js`   | `diff`         | `ok-diff`  | `exit-1`   |
| `require/module-metadata.ts`           | `lib/internal/modules/cjs/loader.js`; `test-module-cache.js` / `test-module-children.js`   | `diff`         | `match`    | `ok-diff`  |
| `require/package-boundaries.ts`        | CJS resolver in `loader.js`; package exports/imports fixtures                              | `compile_fail` | `match`    | `ok-diff`  |
| `require/parent-children.ts`           | `lib/internal/modules/cjs/loader.js`; `test-module-cache.js` / `test-module-children.js`   | `diff`         | `match`    | `ok-diff`  |
| `require/resolve-paths.ts`             | CJS resolver in `loader.js`; package exports/imports fixtures                              | `diff`         | `ok-diff`  | `ok-diff`  |
| `source-map/constructor-payload.ts`    | `test-source-map-api.js`; `lib/internal/source_map/source_map.js`                          | `diff`         | `ok-diff`  | `exit-1`   |
| `source-map/descriptors-validation.ts` | `test-source-map-api.js`; `lib/internal/source_map/source_map.js`                          | `diff`         | `ok-diff`  | `exit-1`   |
| `source-map/find-inline.ts`            | `test-source-map-api.js`; `lib/internal/source_map/source_map.js`                          | `diff`         | `exit-1`   | `exit-1`   |
| `source-map/getter-semantics.ts`       | `test-source-map-api.js`; `lib/internal/source_map/source_map.js`                          | `diff`         | `ok-diff`  | `exit-1`   |
| `source-map/lookup-boundaries.ts`      | `test-source-map-api.js`; `lib/internal/source_map/source_map.js`                          | `diff`         | `ok-diff`  | `exit-1`   |
| `source-map/malformed-indexed.ts`      | `test-source-map-api.js`; `lib/internal/source_map/source_map.js`                          | `diff`         | `ok-diff`  | `exit-1`   |
| `source-map/receiver-validation.ts`    | `test-source-map-api.js`; `lib/internal/source_map/source_map.js`                          | `diff`         | `ok-diff`  | `exit-1`   |

## Rejected or separate-lane candidates

- `stripTypeScriptTypes(..., { mode: "transform" })`: the exact Node 26.5.0
  build rejects the mode; only the supported strip and validation contracts
  remain.
- Compile-cache `FAILED`/`DISABLED` production: forcing them requires
  permissions, read-only/global host paths, or process flags. Constants,
  disabled-state reads, enabling, already-enabled state, directory selection,
  flushing, and cleanup are covered without fabricating failure state.
- Custom generated loader source and runtime-only virtual modules: these
  conflate loader semantics with unsupported AOT dynamic-code evaluation.
  Pass-through registration, validation, hook order, dynamic import invocation,
  and deregistration are covered.
- Symlink/realpath matrices, global search paths, workers, policies/permissions,
  native addons, SEA/snapshots, coverage/inspector, watch mode, races, and
  stress remain in isolated platform/process lanes described in `README.md`.
