# Compose Orchestration (`perry/compose`)

`perry/compose` brings the `docker compose up / down / ps / exec / logs`
workflow into TypeScript. The spec is a TS object literal that mirrors
the [Compose Specification](https://github.com/compose-spec/compose-spec/blob/main/schema/compose-spec.json),
the engine is in-process Rust (no shell-out to a `docker-compose`
binary), and dependency ordering / rollback / interpolation all run
natively.

## Bringing up a single-service stack

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:compose-up-simple}}
```

The handle returned from `up()` is an opaque integer (NaN-boxed with
`POINTER_TAG`); pass it as the first argument to
[`down`](#tearing-down) / [`ps`](#status--logs--exec) /
[`logs`](#status--logs--exec) / [`exec`](#status--logs--exec). The
template-string interpolation `${stack}` renders as `[object Object]`
because of the NaN-boxing tag; coerce explicitly with `String(stack)` if
you need to log it.

## Multi-service stack with healthcheck-gated startup

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:compose-up-multi}}
```

This pattern combines several production-grade primitives:

| Primitive | What it does |
|---|---|
| `container_name: 'app-db'` | Forces a stable container name so docker's embedded DNS resolves `app-db` to the postgres container's IP. **See the [DNS gotcha below](#cross-service-dns-gotcha).** |
| `healthcheck: { test: [...], interval, retries, start_period }` | Per-service liveness probe. Compose-spec § service.healthcheck shape — Perry's engine honors it for `depends_on` gating. |
| `depends_on: { db: { condition: 'service_healthy' } }` | Holds the dependent service back until the dependency reports healthy. Three valid conditions: `service_started`, `service_healthy`, `service_completed_successfully`. |
| `networks: { ..., internal: true }` | Marks the network as internal-only — postgres is unreachable from the host or from sibling stacks. See [Networking](./networking.md). |
| `restart: 'unless-stopped'` | The runtime restarts the container after a crash, but not after an explicit `docker stop`. |

The full `ComposeSpec` shape is exported from `perry/compose` as
`ComposeSpec`, with sub-types `Service`, `ComposeNetwork`,
`ComposeVolume`, `Build`, and `Healthcheck`.

The root-level `name:` field sets the **compose project name** — it
labels every container (`perry.compose.project=<name>`) and namespaces
non-external volumes and networks as `<name>_<declared-name>`, exactly
like docker-compose's project prefix. It defaults to `"perry-stack"`
when omitted, so set it whenever more than one stack can run on the
same host (see [Volumes → Volume naming and
ownership](./volumes.md#volume-naming-and-ownership)).

### Recognised Service fields

The full set Perry's engine understands (matches compose-spec § services):

```typescript,no-test
interface Service {
  image?: string;
  container_name?: string;
  ports?: string[];                                              // "host:container[:proto]"
  environment?: Record<string, string> | string[];               // map or KEY=VALUE list
  labels?: Record<string, string>;
  volumes?: string[];                                            // "host:container[:ro]" or "named:container"
  build?: Build;                                                 // { context, dockerfile, args, … }
  depends_on?: string[] | Record<string, { condition?: string }>;
  restart?: "no" | "always" | "on-failure" | "unless-stopped";
  entrypoint?: string | string[];
  command?: string | string[];
  networks?: string[];
  healthcheck?: Healthcheck;
  user?: string;
  working_dir?: string;
  read_only?: boolean;
  privileged?: boolean;
  cap_add?: string[];
  cap_drop?: string[];
}
```

### `Healthcheck` shape

```typescript,no-test
interface Healthcheck {
  test?: string[];           // ["CMD", "<cmd>", ...] | ["CMD-SHELL", "<line>"] | ["NONE"]
  interval?: string;         // Go duration: "5s", "2m", "1h30m"
  timeout?: string;
  retries?: number;
  start_period?: string;     // grace period before retries count
  disable?: boolean;
}
```

## Environment variable interpolation

Compose's `${VAR}` and `${VAR:-default}` placeholders work in TS-side
specs too — Perry expands them against `process.env` at the FFI
boundary, **before** the JSON gets parsed:

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:env-interpolation}}
```

Set the env vars before invoking your binary:

```bash
NGINX_VERSION=1.27 WEB_PORT=9000 ./my-stack
```

Without this, the literal string `"${NGINX_VERSION:-alpine}"` would
flow through to docker as the image tag and the pull would fail.

## Cross-service DNS

Each service registers its **service key** (`db`, `api`, …) as a
network alias automatically — Perry's engine emits
`--network-alias <key>` per service per network on every `run`. So this
just works:

```typescript,no-test
api: {
  image: "myapp/api",
  environment: {
    // ✅ "db" resolves in DNS via the auto-registered service-key alias
    DATABASE_URL: "postgres://user:pw@db:5432/app",
  },
}
```

`container_name` is no longer required for cross-service DNS. You can
still set one if you want a stable name visible to `docker ps`, but the
service key alone is enough for in-network resolution. Pre-v0.5.372 docs
described a workaround using `container_name` pinning — that pattern
still works but is now optional.

## Tearing down

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:compose-down}}
```

`down(handle)` removes containers and networks, and **preserves named
volumes by default**. Pass `{ volumes: true }` to also drop the volumes
(destroys committed data — use only for "rip and replace" redeploy or
test cleanup). Pass `{ removeOrphans: true }` to also sweep out
containers left behind by earlier deploys of the same project whose
service key no longer exists in the spec.

| `down` option | Type | Default | Effect |
|---|---|---|---|
| `volumes` | `boolean` | `false` | Also remove named volumes after containers + networks. |
| `removeOrphans` | `boolean` | `false` | Remove **orphaned containers**: ones still carrying this stack's `perry.compose.project` label whose `perry.compose.service` key is no longer in the spec (service renamed/deleted between deploys). Strictly label-scoped — other projects' containers and anything not created by Perry are never touched. |

## Status / logs / exec

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:compose-ops}}
```

Like `perry/container.{logs, exec}`, the compose `logs` and `exec`
return registry-id handles for the `ContainerLogs` array. Treat them as
opaque for now; user-side materialisation is a planned ergonomics
task.

| Function | Signature |
|---|---|
| `ps(handle)` | `(handle) → Promise<ContainerInfo[]>` |
| `logs(handle, opts?)` | `(handle, { service?, tail? }) → Promise<ContainerLogs>` |
| `exec(handle, service, cmd[])` | `(handle, service, cmd[]) → Promise<ContainerLogs>` |
| `config(handle)` | `(handle) → Promise<string>` (resolved YAML) |
| `start(handle, services?)` | `(handle, services?: string[]) → Promise<void>` |
| `stop(handle, services?)` | `(handle, services?: string[]) → Promise<void>` |
| `restart(handle, services?)` | `(handle, services?: string[]) → Promise<void>` |
| `down(handle, opts?)` | `(handle, { volumes?, removeOrphans? }) → Promise<void>` |

`exec` targets a service by its **service key** (e.g. `'db'`, not the
container name) — the engine resolves the key to its tracked container
name internally.

## Idempotency

`up()` is idempotent: if a service is already running with a matching
configuration, it's left alone; if it exists but is stopped, it's
`start`ed; only when it doesn't exist at all is it created from
scratch. This makes "redeploy" a no-op-or-restart operation rather
than a tear-down-and-recreate.

> ⚠️ Idempotency works at the **service** granularity, not field-level.
> If you change the spec (e.g. update an image tag), you'll want
> `down(handle, { volumes: false })` followed by `up(newSpec)` so the
> old containers are replaced with the new image.

## Waiting for readiness

`up()` returns as soon as the engine has *started* every service —
not when each service is *ready*. To block until the stack is serving:

1. **Use the `healthcheck` block on the service** (built-in, runtime
   handles it). Combined with `depends_on: { svc: { condition:
   'service_healthy' } }`, dependent services wait for the dependency
   to report healthy.
2. **Run an explicit probe loop in your code.** The
   [Forgejo example](./production-patterns.md) does this for both
   postgres (`pg_isready`) and Forgejo (`/api/healthz` over HTTP), each
   with its own timeout budget.

## Errors and rollback

If any service fails to start, the engine rolls back the entire stack:
every container created during this `up()` call is stopped + removed,
every network created is removed, and (subject to the standard
`session_volumes` semantics) created volumes are removed too. The
returned `Promise` rejects with a `ServiceStartupFailed` containing the
failing service name and the underlying backend error.

```typescript,no-test
try {
  const stack = await up({ /* … */ });
} catch (err: any) {
  // err.message is "Service '<name>' failed to start: <reason>"
  console.error(err);
  process.exit(1);
}
```

## See also

- [Networking](./networking.md) — networks, ports, and the DNS gotcha.
- [Volumes](./volumes.md) — preserving data across `down()`.
- [Production patterns](./production-patterns.md) — case study with
  the Forgejo example.
- [Security](./security.md) — image verification and capability
  isolation.
