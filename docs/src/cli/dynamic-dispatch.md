# Dynamic Stdlib Dispatch (`@perry-allow-dynamic`)

Perry refuses compile-time *dynamic dispatch* on Node-core stdlib namespaces.
A call site like

```typescript,no-test
const m = "exit";
(process as any)[m](0);
```

fails to compile. The check exists to catch the standard string-based
obfuscation pattern used by malicious npm packages:
`process["bind" + "ing"]("dns")`, `globalThis[atob("ZXZhbA==")]()`,
`fs[methodName]()` where `methodName` is computed at runtime.

The pass is purely compile-time — **zero runtime cost** — and is on by
default. Issue [#503](https://github.com/PerryTS/perry/issues/503) tracks the
design.

## What's checked

Dynamic dispatch is refused when **all** of the following hold:

1. The receiver resolves to a known Node-core stdlib namespace:
   `process`, `fs`, `crypto`, `child_process`, `net`, `os`, `path`, `http`,
   `https`, `http2`, `stream`, `url`, `util`, `events`, `dns`, `tls`,
   `querystring`, `zlib`, `async_hooks`, `readline`, `string_decoder`,
   `tty`, `worker_threads`.
2. The index expression is *not* a string literal — `fs["readFileSync"]`
   is treated identically to `fs.readFileSync` and always passes.
3. The user has not opted out (see below).

User-code reflection on user-defined objects is unaffected:

```typescript,no-test
const me = { greet: (n: string) => "hi " + n };
const k = "greet";
me[k]("world"); // ✓ user object, not a stdlib namespace
```

## Opt-outs

The error message lists the available opt-outs in priority order:

### 1. Replace with a static call

The preferred fix. The check exists precisely because static calls are
auditable.

```typescript,no-test
process.exit(0);                // ✓
fs.readFileSync("/tmp/x");       // ✓
```

### 2. `// @perry-allow-dynamic` annotation

For legitimate one-off dispatch, drop a line comment on or immediately
above the offending site:

```typescript,no-test
const k = pickHandler();
// @perry-allow-dynamic
(process as any)[k](0);
```

Contiguous comment lines above the call also count, so the annotation
can sit alongside an `// @ts-ignore` or similar.

### 3. Per-package allow list in `package.json`

To opt one or more npm dependencies out, list them under
`perry.allowDynamicStdlibDispatch` in the **host** application's
`package.json`:

```json
{
  "perry": {
    "allowDynamicStdlibDispatch": ["legacy-dep", "@scope/other-dep"]
  }
}
```

Modules whose source path lives under
`node_modules/<pkg>/…` are matched against this list. Host code is
*not* covered — opting host code out requires the global flag below
or the site annotation.

### 4. Global opt-out

To disable the check across the entire build, set the boolean form:

```json
{ "perry": { "allowDynamicStdlibDispatch": true } }
```

…or set the env var for a one-off build:

```bash
PERRY_ALLOW_DYNAMIC_STDLIB=1 perry build src/main.ts
```

CI can enforce the check by setting `PERRY_ALLOW_DYNAMIC_STDLIB=0`,
which beats any package.json opt-out.

## Why on by default

The check is the cheapest possible defense against the
dispatch-by-string class of supply-chain evasion. The cost to legitimate
code is essentially zero — static calls and literal-keyed access compile
unchanged. Code that genuinely needs the indirection has four ways to
say so explicitly, and the failure mode is a build error rather than a
silent miss in detection.

See [`#503`](https://github.com/PerryTS/perry/issues/503) for design
discussion and the broader supply-chain hardening series ([`#495`–`#506`]
(https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
