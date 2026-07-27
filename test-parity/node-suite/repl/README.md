# `node:repl` parity evidence

This suite contains 68 deterministic, single-contract fixtures for the public
programmatic `node:repl` API. It uses controlled in-memory streams, disables
colors, and never reads from a terminal or user input.

## Fixed sources

| Runtime | Source                                                                                                  | Revision used                                       |
| ------- | ------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| Node.js | `lib/repl.js`, `lib/internal/repl/*`, and the 136 REPL-related files under `test/`                      | v26.5.0, `bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb` |
| Deno    | `ext/node/polyfills/repl.ts`, `ext/node/polyfills/internal/repl.ts`, and `tests/unit_node/repl_test.ts` | `34c46613cbe20450b74c0e8d4f0fd8f6f781d807`          |
| Bun     | `src/js/node/repl.js` and `test/js/node/test/common/repl.js`                                            | `43d60f69c95a9f31591165816ced29b83e94673e`          |
| Perry   | `crates/perry-runtime/src/node_repl.rs` and the API manifest                                            | `563c35951b347aabac3e093efd9c8b2af8ecd5d9`          |

The runnable comparison used Node 26.5.0, Deno 2.9.3, and Bun 1.2.18. Bun's
pinned source has since ported Node's REPL implementation, but the installed
1.2.18 runtime does not yet expose that surface. Source and runtime results are
therefore recorded separately rather than treated as the same claim.

## Coverage

| Directory            | Contracts                                                                                                              | Main Node evidence                                                                                          |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `exports`, `classes` | exports, descriptors, aliases, callable metadata, mode symbols, `Recoverable`, `REPLServer` inheritance and prototype  | `lib/repl.js`, `test-repl-options.js`, `test-repl-recoverable.js`                                           |
| `start`              | object and legacy overloads, stream aliases, defaults, explicit options, custom callbacks, validation                  | `lib/repl.js`, `test-repl-options.js`, `test-repl-unsupported-option.js`                                    |
| `context`            | isolated and global contexts, REPL bindings, context creation, reset state and reset events                            | `test-repl-context.js`, `test-repl-reset-event.js`                                                          |
| `commands`           | built-ins, `.break`, `.clear`, command registration, validation, action receiver and buffered input                    | `test-repl-definecommand.js`, `lib/repl.js` command setup                                                   |
| `eval`               | direct callback results, context, filename, custom callback receiver, syntax recovery, sloppy mode and top-level await | `test-repl-custom-eval.js`, `test-repl-recoverable.js`, `test-repl-mode.js`, `test-repl-top-level-await.js` |
| `output`             | writer defaults, custom writer output, `ignoreUndefined`, disabled colors and prompt methods                           | `test-repl-colors.js`, `test-repl-inspect-defaults.js`, `test-repl-setprompt.js`                            |
| `completion`         | custom completer receiver plus stable built-in, global and context-property completion                                 | `test-repl-tab-complete.js`, `test-repl-tab-complete-custom-completer.js`                                   |
| `history`            | legacy and object setup forms, bounded history, validation and isolated cleanup                                        | `test-repl-programmatic-history.js`, `test-repl-programmatic-history-setup-history.js`                      |
| `lifecycle`          | `exit`, `close`, once-listeners, emit result and exit-before-close order                                               | `test-repl-close.js`, `test-repl-end-emits-exit.js`                                                         |

The top-level-await fixture settles through the REPL eval callback. It uses no
timer or sleep. History fixtures create one private temporary directory and
remove it in `finally` after the setup callback.

## Measured results

Node 26.5.0 passed 68/68 on three runs. Each run produced the same combined
stdout, stderr, exit-code, and fixture-name SHA-256:
`a0e49a26d7348d12b13605321c2d66831a6ba3a70e21f1f9522c92d8eaed1fcc`.

| Runtime     | Pass | Output diff | Error | Compile failure | Crash | Timeout |
| ----------- | ---: | ----------: | ----: | --------------: | ----: | ------: |
| Node 26.5.0 |   68 |           0 |     0 |               0 |     0 |       0 |
| Perry       |   11 |          57 |     0 |               0 |     0 |       0 |
| Deno 2.9.3  |   55 |           9 |     4 |             n/a |     0 |       0 |
| Bun 1.2.18  |    1 |           2 |    65 |             n/a |     0 |       0 |

Perry produced the same 11/68 result on three focused runs. Its stable passes
cover `Recoverable` construction, `REPLServer` construction, `.clear`, custom
command receiver binding, `exit`/once listeners, prompt output,
`ignoreUndefined`, and both `start()` and `new REPLServer()` instance checks.

Perry's remaining differences split into two groups:

- The documented AOT boundary: there is no general JS evaluator, input-driven
  loop, completion engine, or top-level-await evaluator. `write()` only handles
  numeric literals, context number lookups, and one `+` operation.
- REPL API gaps independent of eval: export and prototype descriptors, legacy
  overloads, stream aliases, option validation, full contexts, writers,
  completion hooks, history loading, close behavior, and EventEmitter return
  values.

Deno matches most programmatic contracts. Its stable differences cover its extra
terminal-only `editor` command, context module metadata, top-level-await
callback behavior, export descriptors/keys, history loading/options, and the
removed `domain` option. Its four errors are missing named exports for `writer`
and `isValidSyntax` or the corresponding missing descriptor.

Bun 1.2.18 only matches the built-in-module aliases. Most cases fail at import
time because that release does not expose the named REPL exports. The two
non-error differences are export descriptors and enumerable keys. Current Bun
source contains a newer Node-derived implementation; these runtime results do
not predict that unreleased code's pass count.

## Exclusions and ownership

- Real TTY, editor, reverse search, key handling, previews, paste mode, CLI
  flags, inspector, signals, watch mode, sockets, ports, timing races and pummel
  cases are excluded because they need terminal or process integration.
- `.save` and `.load` execution, interactive history navigation, history file
  permissions and large histories are excluded because their primary contract
  belongs to filesystem or readline behavior. This suite keeps only REPL setup
  and command registration.
- VM execution, context descriptor semantics, console formatting, module
  loading, util inspection, process listeners and readline editing remain owned
  by the `vm`, `console`, `module`, `util`, `process` and `readline` suites.
  REPL fixtures only check that REPL wires those values into its public surface.
- Dynamic imports, referrer paths, stack rendering and require-cache behavior
  are excluded because they mix module resolution or volatile paths/stacks with
  REPL behavior.
- Async exception routing and promise previews are excluded. Their Node tests
  rely on domains, inspector state, process error hooks or timing. The direct
  top-level-await callback is the only async case with a clear barrier.

## Stop criterion

All 136 Node 26.5.0 REPL-related files and the pinned Deno and Bun sources were
reviewed. The remaining upstream cases require an excluded terminal/process
facility, repeat a contract already covered here, belong to another module, or
need runtime eval that Perry explicitly does not claim. Add another fixture only
when upstream exposes a new deterministic, portable, REPL-owned contract.
