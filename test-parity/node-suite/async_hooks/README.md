# `node:async_hooks` granular parity suite

This directory tests deterministic public behavior from `node:async_hooks`.
Fixtures assert ID relationships rather than exact numeric IDs and use explicit
completion barriers for asynchronous work.

## Upstream comparison

The expansion was reviewed on 2026-07-16 against primary repository sources:

- Node.js main at
  [`34c28d5`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/async-hooks),
  especially the AsyncResource lifecycle, AsyncLocalStorage nesting,
  enable/disable, promise, pre-hook Promise creation, late hook activation,
  execution-resource identity, default trigger, concurrent HTTP/socket, and
  async/await cases, plus its
  [bind](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-async-local-storage-bind.js)
  and
  [snapshot](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-async-local-storage-snapshot.js)
  selections.
- Deno main at
  [`f8a17c8`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/unit_node/async_hooks_test.ts),
  whose selected compatibility coverage independently emphasizes nesting,
  enterWith, bind/snapshot, AsyncResource scope callbacks, and async API
  propagation.
- Bun main at
  [`5d350cc`](https://github.com/oven-sh/bun/tree/5d350cc17525a493fcb55b0a014f75af7c414580/test/js/node/async_hooks),
  plus its selected Node compatibility cases for constructor behavior,
  receiver preservation, context isolation, and exit cleanup.

The correctness oracle remains the repository-pinned Node 26.5.0.

The current expansion directly maps the deterministic public contracts from
Node's `test-async-hooks-disable-during-promise.js`,
`test-async-hooks-enable-during-promise.js`,
`test-async-hooks-promise-triggerid.js`, both
`test-promise.*-before-init-hooks.js` cases, `test-late-hook-enable.js`,
`test-nexttick-default-trigger.js`, `test-async-exec-resource-match.js`,
`test-async-hooks-correctly-switch-promise-hook.js`,
`test-async-hooks-close-during-destroy.js`,
`test-async-hooks-execution-async-resource-await.js`,
`test-async-local-storage-http-multiclients.js`,
`test-async-local-storage-socket.js`,
`test-eventemitter-asyncresource.js`, `test-async-wrap-trigger-id.js`,
`test-timers.setInterval.js`, `test-fsreqcallback-readFile.js`,
`test-getaddrinforeqwrap.js`, `test-getnameinforeqwrap.js`,
`test-querywrap.js`, `test-crypto-randomBytes.js`, and
`test-zlib.zlib-binding.deflate.js`, `test-immediate.js`,
`test-fseventwrap.js`, `test-statwatcher.js`, `test-udpwrap.js`,
`test-tcpwrap.js`, `test-shutdownwrap.js`, `test-graph.timeouts.js`,
`test-pipewrap.js`, `test-signalwrap.js`, and
`test-async-wrap-providers.js`, plus the `DIRHANDLE` selection in
`test-async-wrap-getasyncid.js`. The current fixtures additionally isolate
`test-timers-clearImmediate-als.js`, the AsyncLocalStorage branch of
`test-web-locks.js`, the resource back-reference from
`test-eventemitter-asyncresource.js`, and the bounded WORKER/MESSAGEPORT
relationships exercised by Node's worker provider selections. The provider
request fixtures also exercise the
`HASHREQUEST`, `CIPHERREQUEST`, and `SIGNREQUEST` paths used by Node's current
crypto implementation. Deno's selected bind/snapshot, nesting, enterWith,
resource-scope, and propagation contracts and Bun's async-context provider
matrix are represented by smaller single-boundary fixtures rather than copied
monolithic tests.

For the latest 12 new fixtures and three enhanced fixtures, Node 26.5.0 produced
byte-identical output in three rounds. The additions cover the complete claimed
function/export metadata, module namespace descriptors and immutability,
constructor-call and inherited option-accessor behavior, accessor exception
cleanup, arbitrary `enterWith` store values, self-cleared immediates, Web Locks,
EventEmitterAsyncResource back-references, BLOBREADER, and DNSCHANNEL. Existing
HTTP, worker, and timers/promises cases now also cover native provider
lifecycles, scheduler wait/yield, and promise intervals. Perry matches the new
Web Locks and `enterWith` value matrices; the other ten new cases remain stable
diagnostics. Bun 1.2.18 matches the module surface, constructor calls,
self-cleared immediates, value matrix, and timers additions but still exposes a
no-op `createHook`. Deno 2.9.2 matches the module surface, constructor calls,
hook option access/lifecycle, self-cleared immediates, value matrix,
EventEmitterAsyncResource back-reference, and timers additions; its provider
selection remains different. These divergences are comparison evidence, not a
reason to weaken the Node oracle.

For the preceding 12 fixtures, Node 26.5.0 produced byte-identical output in three
rounds. Deno 2.9.2 matches Node's import/namespace table, both subclass cases,
nested Timeout lifecycle, AsyncResource detached methods, and frozen
null-prototype provider table, while its native provider selection differs for
child pipes, signals, directory handles, and WebCrypto requests. Bun 1.2.18
matches import identity/branding and AsyncLocalStorage subclass propagation,
but its current `createHook` is a no-op and its provider table remains mutable
with an ordinary object prototype. These divergences are recorded as comparison
evidence, not used to weaken the Node oracle.

The current focused result is **78/193** and is recorded in
`node_suite_baseline.json`. The suite keeps every stable mismatch as a diagnostic
rather than removing unsupported cases: failures identify context loss, missing hook callbacks/resources,
lifecycle differences, validation gaps, or a compile/runtime boundary for the
specific provider named by the fixture.

The 115 non-matching diagnostics are stable and grouped as follows:

- hook delivery/configuration: custom and built-in provider lifecycle callbacks,
  cancelled resource destruction and identity, simultaneous hooks, late
  activation during timers/immediates/next ticks and Promise chains,
  pre-created Promise relationships, mixed Promise hook shapes, destroy work
  queued from a destroy callback, repeated interval and sibling-nextTick
  resources, fs.readFile/fs-promises and DNS trigger/lifecycle resources,
  filesystem watcher, DIRHANDLE, BLOBREADER, DNSCHANNEL, PROCESS/PIPE, SIGNAL,
  WORKER/MESSAGEPORT, HTTP client/incoming, UDP/TCP/shutdown,
  classic and WebCrypto request, randomBytes, and zlib resources,
  `promiseResolve`, resource arguments, execution-resource mapping/metadata,
  static-bind resource types, the async-wrap provider table prototype, and
  `trackPromises` behavior/validation;
- scheduling/context: zlib, HTTP/HTTPS keep-alive reuse and concurrent clients,
  net callback/data isolation, dgram, subprocess, worker, VM, dynamic import,
  readline, events.on, and stream.finished boundaries;
- callback contract: several async crypto APIs invoke their callback before the
  call returns, while prime callbacks do not settle;
- resource/storage semantics: AsyncResource and AsyncLocalStorage native-class
  subclassing, constructor-call behavior, option getter access/exception
  cleanup, detached-method receivers, reflected API metadata, module namespace
  descriptors/immutability, self-cleared Immediate metadata, EventEmitterAsyncResource
  back-references, snapshot receiver handling, top
  execution-resource restoration, disable cleanup, caught async `exit()`
  rejection routing, module namespace branding, and EventEmitterAsyncResource
  prototype/getter brand behavior; and
- runtime: after a clean Perry compiler/runtime rebuild, the direct `node:tls`
  fixture compiles but its local TLS connection does not settle within the
  granular runner's 30-second execution limit. The same certificate fixture
  passes the pinned Node oracle.

## Coverage

- `resource/`: construction/type and ID invariants, scope/receiver/arguments,
  instance/static bind including inferred resource types and explicit receiver,
  option getter order and inherited/null-prototype options, detached-method
  receiver behavior, native-class subclass identity,
  deterministic hook scope callbacks, and explicit destroy.
- `storage/`: run nesting and restoration, independent instances, enterWith,
  exit and its async descendants, EventEmitter listener bleed/isolation,
  multiple store value types, cross-instance exit isolation, disable/re-entry,
  repeated disable, all primitive/object `enterWith` store values, self-cleared
  Immediate context/handle identity, detached/foreign method receivers,
  native-class subclass propagation, and promise-boundary behavior.
- `static/`: AsyncLocalStorage bind/snapshot and AsyncResource.bind context,
  empty and populated captures, receiver, argument, return-value, re-entry, and
  restoration behavior.
- `propagation/`: controlled concurrent promises, catch/finally, thenables,
  thenables returned from async functions and handlers, async iterators,
  dynamic import, local fetch, Web Locks, VM scheduling, queueMicrotask, nested nextTick,
  immediate, ref/unref, interval, and timer propagation with awaited or
  callback-driven completion barriers.
- `integrations/`: individual fs access, mutation, metadata, descriptor,
  directory, watch, promises, and stream callbacks; crypto random, KDF, key,
  key-pair, and prime callbacks; all major zlib callback/stream families;
  Readable/Writable/Transform/finished, timers/promises timeout/immediate/
  interval plus scheduler wait/yield, util.promisify,
  util.promisify.custom, EventEmitter, and EventEmitterAsyncResource lifecycle
  plus prototype-brand behavior selected from Bun's async-context matrix and
  Node's provider tests.
- `hooks/`: enable/disable/re-enable, simultaneous observers, enabling and
  disabling observers from `init`, `before`, and `after` or while callbacks and
  Promise chains are active, mixed Promise hook shapes, pre-created and
  async/await Promise trigger chains, execution-resource identity and writable
  metadata propagation, default next-tick triggers, re-entrant destroy queuing, `trackPromises`,
  `promiseResolve`, resource arguments, cancelled timer/immediate destruction,
  deterministic timer/interval/immediate/microtask/nextTick, nested Timeout
  ancestry, callback and promise fs, filesystem watcher and directory handles,
  classic/WebCrypto requests, PBKDF2/randomBytes, zlib, BLOBREADER, DNS plus
  DNSCHANNEL, child-process PROCESS/PIPE, signal registration, UDP, and local
  TCP lifecycles; sibling,
  fs.readFile, and provider trigger ancestry; async-wrap provider-table
  immutability; and throwing scoped callbacks.
- `providers/`: DNS, child processes, HTTP and HTTPS including keep-alive agent
  and concurrent-client isolation, HTTP execution-resource mapping, TLS, net
  including concurrent data, dual accept/connect context isolation, and
  `getConnections`; dgram including dual send/receive context isolation; DNS
  `lookupService`; workers, readline, events.on, and stream async iterators with
  local endpoints, ephemeral ports, and explicit close/exit barriers. The HTTP
  and worker cases also assert HTTPCLIENTREQUEST/HTTPINCOMINGMESSAGE and
  WORKER/MESSAGEPORT execution-resource mapping and balanced lifecycles. This
  directory runs in the sequential lane.
- `validation/`: synchronous throw propagation and cleanup of storage and
  execution-resource state, constructor-call behavior, option-accessor
  exception short-circuiting, plus hook-sensitive empty resource type behavior.
  The pre-existing root fixtures retain detailed callback, constructor, and
  hook-option argument validation.
- root module fixtures: bare/prefixed/default import identity, complete claimed
  export/function metadata, and ESM namespace branding/descriptors/immutability
  are isolated so a correct export identity does not hide namespace mismatches.

## Remaining slow, redundant, or environment-sensitive categories

The following current upstream selections are not counted as coverage. Each is
kept out for a concrete reason rather than to cap the suite size:

- Exact numeric async IDs are runtime-specific; only relationships and
  restoration invariants are asserted here.
- GC-driven destroy delivery, weak-reference collection, destroy-vs-scheduler
  priority, recursive hooks, deep stacks, stress/leak probes, and process
  shutdown require slow or timing-sensitive runners.
- Cross-worker storage inheritance is not a Node contract; the retained worker
  cases instead check the parent-side `online`, `message`, and `exit` provider
  callbacks plus bounded WORKER/MESSAGEPORT lifecycles with an explicitly
  terminated local worker. A separate direct MessageChannel lifecycle probe was
  rejected because Bun 1.2.18 kept the process alive past 30 seconds even after
  listeners were removed and both ports were unrefed and explicitly closed.
- Uncaught exception and unhandled rejection routing mutates process-global
  handlers and belongs in a dedicated isolation fixture.
- Bun's DNS CNAME/MX/TXT/reverse cases depend on external resolver state. Local
  `lookup`, `resolve4`, and the Node lifecycle selections cover the stable DNS
  provider boundary without making network availability an oracle input.
- Bun's crypto cipher/hash/sign/randomUUID selections perform their work
  synchronously and only check a following `setImmediate`; that scheduling
  behavior is already isolated by the immediate propagation and hook fixtures.
- Node's HTTP parser/socket reuse graphs, exhaustive provider topology, signal
  delivery/re-registration, local-domain PIPECONNECT/PIPESERVER graphs, forced
  WRITEWRAP backpressure, TTY, process-shutdown, and inspector/trace-event cases
  assert native implementation details or depend on a separate transport
  boundary. The retained bounded cases cover child stdio PROCESS/PIPE resources,
  signal registration/removal, and the stable loopback TCP relationships
  selected by Node's own tests. A local-domain socket probe currently fails in
  Perry with `Can't assign requested address (os error 49)`, while the forced
  8 MiB WRITEWRAP probe leaves its write callback unsettled past 30 seconds.
- Node's HTTP/2 ALS selection cannot yet reach an async-context callback in
  Perry because its local plaintext client never emits `connect`; it belongs
  after the underlying `node:http2` provider can complete a loopback session.
- Bun's ReadableStream `cancel` selection expects captured context while Node
  26.5.0 reports `undefined`. Its `pull` selection matches Node, but activates a
  separate Web Streams feature whose cold Perry build exceeds the granular
  runner's compile budget; both belong with Web Streams compatibility rather
  than this Node module suite.
- Bun's cipher-stream context selection cannot currently reach its ALS
  assertions because Perry's base `createCipheriv` stream never emits data/end
  and exits with unsettled top-level await; it belongs after crypto stream event
  support is functional.
- Node's ARGON2REQUEST and prime-request providers are not retained yet: the
  corresponding Perry async `argon2`, `checkPrime`, and `generatePrime`
  callbacks do not settle, so provider assertions cannot be reached without
  first fixing the separate `node:crypto` callback boundary.
- Node's `UDPSENDWRAP` selection requires the internal
  `--test-udp-no-try-send` flag to disable the synchronous fast path. Without
  that non-public runner flag Node 26.5.0 emits no send resource, so the portable
  suite retains UDP socket lifecycle and dgram callback-context coverage only.
- A current Node-main test expects the removed `AsyncResource.domain` getter,
  but the pinned Node 26.5.0 oracle has no such prototype getter; that
  post-oracle surface is not counted.
- Node 26.5.0 supports AsyncLocalStorage `defaultValue`, `name`, and `withScope`,
  but Perry's API manifest does not claim those constructor/property/scope
  surfaces, so they are not counted here.

Raw hook coverage remains deliberately bounded. User-created AsyncResource
cases control init/before/callback/after/destroy directly. Selected built-in
provider cases are retained only where the pinned Node implementation or suite
supplies a stable contract and the operation is local and deterministic:
interval and nextTick,
fs.readFile and fs promises/watchers, Blob reads, DNS channel plus
lookup/lookupService/localhost query, directory handles, randomBytes and other
classic/WebCrypto requests, zlib, child stdio, bounded worker and HTTP
providers, signal registration, UDP socket, and a bounded loopback TCP matrix.
All use explicit completion barriers and assert relationships rather than raw
IDs; GC timing and exhaustive native topology graphs remain excluded.
