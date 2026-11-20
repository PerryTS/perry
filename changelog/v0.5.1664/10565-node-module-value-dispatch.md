Fixed `node:net` and `node:http`/`https`/`http2` exports being inert whenever the module object was
used as a VALUE: a CommonJS `require('net')`, an aliased namespace, a destructured or pulled-out
export (`(0, net_1.createConnection)(opts)`), or `new` on a bound class value. `connect()` /
`createConnection()` / `createServer()` / `request()` returned `undefined` under
`PERRY_NO_AUTO_OPTIMIZE=1` and, for any net-only program, in both compile modes; `net.isIP` was never
callable as a value, `new (net.Socket)()` produced a plain object, and
`require('node:http') !== require('http')`. These are the shapes pg, mysql2, ioredis/iovalkey, redis,
ws and fastify use when compiled from source (#10428, #10429).

Root cause: the runtime's module-object dispatch forwarded these exports to a callback that only
perry-stdlib registered, behind the `external-http-server-pump` feature — deliberately absent from
the prebuilt `full` archive and enabled by auto-optimize only for programs importing
http/https/http2. The dispatcher lived in perry-stdlib while every implementation it routes to lives
in perry-ext-http / perry-ext-net, which is why it had to be gated at all.

The dispatchers now live in the provider crates (`perry-ext-net/src/native_dispatch.rs`,
`perry-ext-http/src/server/native_dispatch.rs`) and register themselves from their namespace install
symbol and from the entry prologue, which calls the install wrapper of every linked provider before
module initialization — so module objects the runtime creates itself (a CommonJS `require` resolving
through `createRequire`) reach them too. A dedicated `JS_NATIVE_NET_DISPATCH` hook carries the net
exports, `js_nm_install_net` registers a constructor arm for `Socket`/`Stream`/`Server`/`BlockList`/
`SocketAddress`, the `isIP` family joined the callable-export table, the http/https/http2/net
namespaces are cached so both spellings are one object, and perry-ext-http now registers handle
dispatch extensions for ClientRequest / client IncomingMessage / Agent so erased receivers work
without `external-http-client-pump` (the client twin of the Wall-10 server-handle fix).

Validated with a new gap test covering every value shape against in-process servers (identical to
Node in BOTH compile modes; the baseline diverges in both), runtime/codegen/CLI unit tests, the full
gap suite (no new failures), and instruction counts on direct `net.connect` and `http.request` loops
(±0.06%, static call path untouched).
