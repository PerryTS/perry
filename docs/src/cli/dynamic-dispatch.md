# Dynamic Stdlib Dispatch (`--lockdown`)

Perry can refuse compile-time *dynamic dispatch* on Node-core stdlib
namespaces. A call site like

```typescript,no-test
const m = "exit";
(process as any)[m](0);
```

is the standard string-based obfuscation pattern used by malicious npm
packages: `process["bind" + "ing"]("dns")`, `globalThis[atob("ZXZhbA==")]()`,
`fs[methodName]()` where `methodName` is computed at runtime.

**Default: allowed.** Since [#5263](https://github.com/PerryTS/perry/issues/5263)
the refusal is *off* by default. Dynamic `fs[name]` over a namespace Perry has
already statically linked can only *select among the methods that were linked*
— it is dynamic selection of a known set, not a way to reach arbitrary code —
so it is safe to allow, and legitimate packages depend on it (graceful-fs
stores its retry queue on `fs[Symbol.for('graceful-fs.queue')]`; fs-extra wraps
the known `fs[method]` functions). Dynamic reads resolve the linked member by
name; writes the program performs persist and read back — **string** keys via a
module-keyed override side-table, **symbol** keys on the cached namespace object
(so graceful-fs's `fs[Symbol.for('graceful-fs.queue')] = queue` round-trips).

**The refusal is re-armed by [`--lockdown`](./lockdown.md)** — the
supply-chain gate — and by an explicit opt-out (below). Under those, the
site below fails to compile with `error[U006]`. The pass is purely
compile-time (**zero runtime cost**). Issue
[#503](https://github.com/PerryTS/perry/issues/503) tracks the original design.

## What's checked (when the refusal is armed)

The refusal is armed under `--lockdown` (or `perry.lockdown: true` /
`PERRY_LOCKDOWN=1`), or by an explicit `perry.allowDynamicStdlibDispatch: false`
/ `PERRY_ALLOW_DYNAMIC_STDLIB=0`. When armed, dynamic dispatch is refused when
**all** of the following hold:

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

## Opt-outs (when armed)

When the refusal is armed and a site is refused, the error message lists the
available opt-outs in priority order:

### 1. Replace with a static call

The preferred fix. The check exists precisely because static calls are
auditable.

```typescript,no-test
process.exit(0);                // ✓
fs.readFileSync("/tmp/x");       // ✓
```

### 2. `// @perry-allow-dynamic` annotation (host code only)

For legitimate one-off dispatch in your own code, drop a line comment
on or immediately above the offending site:

```typescript,no-test
const k = pickHandler();
// @perry-allow-dynamic
(process as any)[k](0);
```

Contiguous comment lines above the call also count, so the annotation
can sit alongside an `// @ts-ignore` or similar.

The annotation is honored **only in host source files** (anything not
under `node_modules/`). A dependency cannot grant itself the opt-out by
writing `// @perry-allow-dynamic` next to its own call — that would
defeat the supply-chain defense the check exists for. Dependencies opt
in via the host's per-package allow list (below) or the global flag.
Tracked in [#996](https://github.com/PerryTS/perry/issues/996).

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

## Why allowed by default (and gated under lockdown)

Dynamic member access over a *linked* stdlib namespace can only reach the
methods Perry already linked statically — it is dynamic *selection* among a
known set, not a way to construct or reach arbitrary code. The
dispatch-by-string obfuscation it could otherwise hide is only meaningful when
paired with the arbitrary-code surfaces that `--lockdown` already forbids
(`eval`/`Function`, `child_process`, native archives). So the refusal belongs
with the rest of the lockdown gate, not always-on: default builds allow it (so
graceful-fs, fs-extra, and similar legitimate patterns compile), while
security-sensitive builds opt into `--lockdown` and get the refusal back. An
explicit `perry.allowDynamicStdlibDispatch: false` / `PERRY_ALLOW_DYNAMIC_STDLIB=0`
re-arms just this check without the rest of lockdown.

See [`#503`](https://github.com/PerryTS/perry/issues/503) for design
discussion and the broader supply-chain hardening series ([`#495`–`#506`]
(https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
