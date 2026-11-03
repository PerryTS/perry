Follow-up to #7726 (issue #7720). A 32-case A/B matrix over the node-core spread
surface — this tree vs. the same tree with the guard forced to `false`, both
against node 26.5.1 — found two calls that were correct before #7726 and
`undefined` after, plus two wrong-to-differently-wrong conversions. The matrix is
now 17 fixed / 14 same / 1 wrong-to-wrong / **0 regressed**.

Completed the `node:querystring` runtime bridge. `nm_dispatch_querystring`
advertises `escape`/`unescape`/`stringify`/`encode`/`parse`/`decode` alongside
`unescapeBuffer`, but the stdlib function it calls
(`js_querystring_native_dispatch`) implemented only `unescapeBuffer` and returned
`undefined` for the rest. That made every INDIRECT form silently `undefined` long
before spread calls were routed there — `const e = qs.escape; e("a b")` and
`const d: any = qs; d.escape("a b")` both produced `undefined` while the
statically dispatched `qs.escape("a b")` was correct. The `js_querystring_*`
entry points all existed; only the bridge rows were missing (`encode`/`decode`
are Node's aliases for `stringify`/`parse`).

Narrowed the #7726 sub-namespace rule to the DOTTED tags the runtime dispatcher
actually has a bucket for (`path.posix`, `path.win32`, `util.types`,
`crypto.subtle`/`webcrypto`, `punycode.ucs2`) instead of deriving it from
`NODE_BUILTIN_MODULES`. There is no `fs.promises` bucket, so the derived rule
diverted `dns.promises.lookup(...args)` into a silent `undefined` and
`import { promises } from "node:fs"; promises.readFile(...args)` into a
synchronous `TypeError: value is not a function` where a rejected promise used to
arrive. The slash sub-module tags are rejected too: the direct import
(`import fsp from "node:fs/promises"`) already reaches the generic tail without
the bail, measured identical on both arms.

Known and deliberate: `events.listenerCount(...args)` still turns a bogus
`ERR_INVALID_ARG_TYPE` throw into `undefined` — `nm_dispatch_events` implements
only `init` and `EventEmitterAsyncResource`, so there is no arm to reach. Both
are wrong (node returns a count); completing that dispatcher is its own change.
