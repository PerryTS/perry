# Privacy & Telemetry

Perry ships **two independent opt-in channels** for sending data home —
nothing leaves your machine without an explicit `enabled = true` or `on` in
`~/.perry/config.toml`. Both honour `PERRY_NO_TELEMETRY=1` and `CI=true`.

## 1. Generic usage analytics — `telemetry.enabled`

Counts `perry compile`, `perry init`, `perry publish` invocations on a
background HTTP POST. Sends: command name, platform (`darwin`/`linux`/...),
Perry version, success/error status, and an anonymous client UUID.

## 2. Compatibility reports — `telemetry.compatibility_reports` (#849)

Separate opt-in for "I hit an unsupported TS/Node feature and bailed." Sends
a structured report when the compiler emits one of these diagnostic codes:
`UnsupportedBinaryOp`, `UnsupportedExpression`, `UnsupportedStatement`,
`DynamicPropertyAccess`, `ImplicitCoercion`, `UnresolvedImport`, `NoOpStub`.

Three modes:

- `off` — never send. Sink isn't even installed; zero overhead.
- `ask` (default) — when a qualifying diagnostic fires, prompt once per
  session: `[y] just this once / [a] always / [n] not this time / [N] never`.
- `on` — always send (after dedup + redaction). No prompt.

**What's sent (the entire payload schema):**

```json
{
  "perry_version": "0.5.x",
  "client_id": "uuid",
  "code": "UnsupportedExpression",
  "category": "gap-categorical",
  "stage": "hir-lower",
  "snippet_hash": "sha256:...",
  "snippet_redacted": "let <id1> = await <id2>();",
  "ts_feature": "decorator",
  "node_api": "node:async_hooks.createHook",
  "os": "darwin-arm64",
  "node_target": "20"
}
```

**What's NEVER sent:** raw source, file paths, project names, env vars, your
program's stdout/stderr, dependency tree, or anything tied to identity
beyond the existing anonymous `client_id`. Snippets are redacted before
hashing — string literals → `"<str>"`, numbers → `<num>`, identifiers
(except built-ins like `console`, `Math`, `Promise`) → `<id1>`, `<id2>`,
capped at 200 chars, with a hard reject if any invariant fails.

A local 30-day dedup cache at `~/.perry/.report-cache` prevents resending
the same `snippet_hash` on every reload.

## Inspecting & managing

```bash
perry doctor                          # shows current mode, sent/queued counts
perry doctor --show-pending-reports   # print redacted payloads queued this run
perry doctor --clear-report-cache     # wipe the 30-day dedup cache
```

To opt out at the file level, edit `~/.perry/config.toml`:

```toml
[telemetry]
enabled = false                  # generic analytics off
compatibility_reports = "off"    # #849 compat reports off
```

See also the [`PERRY_NO_TELEMETRY` row in the perry.toml reference](perry-toml.md).
