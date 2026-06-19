# Compile-Time Egress Allowlist (`perry.allowedHosts`)

Perry can verify, at compile time, that every outbound network call
in your binary targets a host you've explicitly approved. When the
host application opts in via `perry.allowedHosts` in `package.json`,
every literal URL/host in a `fetch(...)`, `net.connect(...)`, or
`net.createConnection(...)` call must match one of the listed
patterns — otherwise the build fails before producing a binary.

**Zero runtime cost.** The check runs at compile time over the
lowered HIR. The resulting binary is the same size and shape as a
build without the gate.

## Why a compile-time check

Runtime allowlists are foot-shoots — a misconfiguration or a malicious
dep can bypass them. A compile-time check gives a stronger property:
`grep`-ing the binary's egress is reliable. If a dep tries to add a
new outbound host through a literal URL, the build fails and the
review catches it; if it tries to hide the host behind a variable,
the build still fails unless you've explicitly opted into dynamic
hosts.

## Configuration

In your host `package.json`:

```json
{
  "perry": {
    "allowedHosts": [
      "api.example.com",
      "*.cdn.example.com",
      "https://api.acme.com/v1/*"
    ]
  }
}
```

### Pattern syntax

- **Exact host** — `"api.example.com"` matches that hostname on any
  scheme/port/path.
- **Subdomain wildcard** — `"*.cdn.example.com"` matches every direct
  or transitive subdomain. The bare suffix does NOT match — `*.foo.com`
  does not match `foo.com`.
- **URL prefix** — `"https://api.acme.com/v1/*"` matches any URL
  starting with that literal prefix. Path-bound entries only gate
  path-bearing call sites — `net.connect("api.acme.com")` against a
  URL-prefix entry does NOT match (use a host-style entry for that).
- **Universal** — `"*"` matches everything (escape hatch for
  incremental migration; defeats the static guarantee).

## Dynamic URLs / hosts

Non-literal arguments — `fetch(someVar)`, `net.connect(port, hostVar)`,
template strings with substitutions — defeat the static `grep`-the-binary
guarantee. They're refused by default:

```typescript,no-test
const url = "https://api.example.com/x";
const resp = await fetch(url); // refused unless allowDynamicHosts: true
```

To allow them, set `perry.allowDynamicHosts: true`:

```json
{
  "perry": {
    "allowedHosts": ["api.example.com"],
    "allowDynamicHosts": true
  }
}
```

The code reviewer then has to trust the value of every variable that
reaches `fetch(...)` — explicit acknowledgment that the static
guarantee is being weakened.

## Opt-in semantics

If `perry.allowedHosts` is **not set**, the entire pass is disabled
and existing builds compile unchanged. The host opts in by setting
the array; once set, the gate is strict.

This is intentionally not "default-deny on greenfield" — that would
break every existing build that calls `fetch(...)`. Migration path:

1. Run the build once without the allowlist.
2. Inspect `audit.json` in the cache dir (default
   `node_modules/.cache/perry/audit.json`) (the [behavioral SBOM
   (`#495`)](perry-audit-sbom.md)) and see what egress the binary
   currently performs.
3. Populate `allowedHosts` with the surface you actually use.
4. Re-build. The gate now catches future regressions.

## Diagnostic shape

The build fails with one combined diagnostic naming every offending
site at once (better UX than failing on the first one and asking the
user to re-run):

```text
Error: egress allowlist refused 2 call site(s):
  - /repo/main.ts: fetch → "https://evil.com/leak" (literal host not in `perry.allowedHosts`)
  - /repo/lib/foo.ts: net.connect → "x.evil.com" (literal host not in `perry.allowedHosts`)

`perry.allowedHosts` provides a static guarantee that this binary's
outbound network surface matches the declared list. Refusing the build. (#502)

Options:
- Add the offending host(s) to `perry.allowedHosts` ...
- Set `"*"` in `allowedHosts` to disable host gating ...
- For non-literal URLs, set `perry.allowDynamicHosts: true` ...
```

The list is capped at 12 entries so pathological builds don't produce
60-line errors; trailing sites are summarised as `... and N more`.

## What's covered now

This first cut covers the highest-volume egress shape: `fetch(...)` +
`net.connect(...)` / `net.createConnection(...)`. Other shapes —
`http.get(...)`, `https.request(...)`, `WebSocket(...)` — lower
through the general-shape `NativeMethodCall` HIR variant and will
graft onto the same pass in a follow-up.

## See also

- [`#502`](https://github.com/PerryTS/perry/issues/502) — design discussion.
- [`perry audit --sbom`](perry-audit-sbom.md) (#495) — discover what
  egress your binary currently performs before populating the
  allowlist.
- The wider supply-chain hardening series
  ([`#495`–`#506`](https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
