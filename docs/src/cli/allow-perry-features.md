# Host Allowlist for `nativeLibrary` and `compilePackages`

Perry refuses to honor two privileged dependency features — the two
attack surfaces Perry itself introduced over Node — unless the host
application has explicitly opted in to each consumer:

- `perry.nativeLibrary` — a transitive dep linking arbitrary native
  code into the binary.
- `perry.compilePackages` — compiling untrusted TS source from an npm
  package into the binary as if it were first-party code.

Both checks fire at compile time. **Zero runtime cost.**

## How a build hits this

### `nativeLibrary` (transitive dep declares it)

A package shipped with `perry.nativeLibrary` in its own `package.json`
is detected during dependency collection. Without an entry in the
host's `perry.allow.nativeLibrary`, the build fails:

```text
Error: package `@bloomengine/engine` declares `perry.nativeLibrary`
(links arbitrary native code into the binary) but is not in your host
`perry.allow.nativeLibrary`. Review the package, then add it to your
host `package.json`:

  {
    "perry": {
      "allow": { "nativeLibrary": ["@bloomengine/engine"] }
    }
  }
```

### `compilePackages` (host or workspace root declares it)

Every entry in `perry.compilePackages` must also be matched by an
entry in `perry.allow.compilePackages` — a two-key opt-in:

```text
Error: package `hono` is in `perry.compilePackages` but not in
`perry.allow.compilePackages` — compiling untrusted TS into the binary
is a privileged operation and requires explicit host opt-in. (#497)
```

## Opt-in mechanisms

### 1. Host `package.json` (persistent, recommended)

```json
{
  "perry": {
    "compilePackages": ["hono"],
    "nativeLibrary": "...",
    "allow": {
      "compilePackages": ["hono"],
      "nativeLibrary": ["@bloomengine/engine"]
    }
  }
}
```

### 2. Scope wildcard

`"@scope/*"` matches any package under `@scope/`:

```json
{
  "perry": {
    "allow": {
      "compilePackages": ["@nestjs/*", "reflect-metadata", "rxjs"]
    }
  }
}
```

### 3. Universal escape hatch

`"*"` matches every name. Use sparingly — defeats the purpose of the
allowlist.

```json
{ "perry": { "allow": { "compilePackages": ["*"] } } }
```

### 4. Environment variable

`PERRY_ALLOW_PERRY_FEATURES=1` opts every package into both
allowlists for the current build — emergency knob for one-off builds
where editing `package.json` isn't an option. `=0` enforces refusal
even when `package.json` opted in (fail-closed CI gate).

## Default-deny rationale

Both features escape Perry's structural guarantees:

- `nativeLibrary` lets a transitive dep ship arbitrary machine code
  that runs at the same trust level as the host application.
- `compilePackages` runs the dep's TypeScript through Perry's full
  native pipeline (HIR / codegen / linker) instead of routing it
  through QuickJS, eliminating the runtime sandbox.

Both are useful features, but they're *privileged* operations. The
allowlist makes that privilege explicit and auditable: a reviewer
diffing a PR can see exactly which deps have been granted native
access, and `git blame` records who approved each one.

## See also

- [`#497`](https://github.com/PerryTS/perry/issues/497) — design discussion.
- The wider supply-chain hardening series
  ([`#495`–`#506`](https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
