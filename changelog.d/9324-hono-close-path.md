### Fixed

- **A `TypeError: is not iterable` now names its subject, and `WebSocketServer.clients` resolves for an untyped receiver (#9324).**

  A Hono service on `@hono/node-server` exited every 30 seconds under systemd
  with an unhandled

  ```text
  TypeError: is not iterable
      at node_modules/@hono/node-server/dist/index.mjs:595
  ```

  Line 595 is the adapter's response `"close"` handler, so the report — and the
  issue title — blamed the close path. It was not the close path. Two separate
  defects combined to hide the real line:

  **1. The TypeError named nothing.** Node *never* throws a subject-less
  `is not iterable`; it always names the receiver (`undefined is not iterable`,
  `null is not iterable`, `37 is not iterable`). Perry's synchronous
  `GetIterator` threw the bare string, so the message carried no information at
  all about what had failed, and the only frame available pointed somewhere
  else. `crates/perry-runtime/src/symbol/iterator.rs` now builds the message
  from the value, using the same `null` / `undefined` / `value` label scheme the
  async twin (`array::iterator::throw_not_iterable`) has always used. Three of
  the four shapes below were bare before; `null`, the array destructure and the
  spread now match Node byte for byte:

  | expression | before | after | Node 26.5.1 |
  |---|---|---|---|
  | `for (… of holder.clients)` (missing) | `is not iterable` | `undefined is not iterable` | `holder.clients is not iterable` |
  | `for (… of null)` | `is not iterable` | `null is not iterable` | `null is not iterable` |
  | `const [a] = undefined` | `is not iterable` | `undefined is not iterable` | `undefined is not iterable` |
  | `[...undefined]` | `undefined is not iterable` | `undefined is not iterable` | `undefined is not iterable` |

  **2. The actual throw was `for (const ws of wss.clients)`** in a 30-second
  `setInterval` WebSocket heartbeat — which is why the journal shows the exit
  exactly 30 s after every `listening on :3000`, on a fixed period, with no
  load correlation at all. #9325/#9335 made `clients` a real `Set`, but only
  through the statically-typed lowering, which needs the receiver's class at
  compile time. Every *untyped* read still produced `undefined`: an `any` alias,
  a computed `wss[key]`, a helper taking the server as an untyped parameter, and
  — the case that matters — every compiled npm package, since a published
  bundle carries no types. Neither ws provider registered any handle-property
  surface, so both are fixed: `perry-stdlib`'s bundled twin gains a `clients`
  arm in `js_handle_property_dispatch`, and `perry-ext-ws` (which the
  workspace-rebuild flip routes to) gains a handle-property dispatch extension
  alongside its GC root scanner. Both return "not handled" for any other
  property and for any handle that is not a live `WsServerHandle`, so every
  other handle family falls through unchanged.

  Verified on Linux x86_64 against the exact reported dependency tree
  (`@hono/node-server` 1.19.17, hono 4.13.4, the reporting service's
  `new WebSocketServer({ noServer: true })` shape). On the build the issue was
  filed against the heartbeat reproducer exits 1 with the bare message; after
  the fix it survives on *both* ws providers. The `@hono/node-server` response
  close path — the frame the issue named — needed no change: instrumenting the
  adapter's own `"close"` listener under rapid POST and early-client-disconnect
  load showed it firing 30/30 times on completed responses with
  `writableFinished === true`, taking neither `abort()` branch and throwing
  nothing.

  Regression coverage: `crates/perry/tests/issue_9324_not_iterable_names_its_subject.rs`
  (both halves end to end; the dynamic-read half failed on this very test before
  the `perry-ext-ws` change landed), plus always-on unit tests for the message
  label (`perry-runtime`) and the dispatch extension (`perry-ext-ws`, including
  that a dropped server handle is *not* claimed).
