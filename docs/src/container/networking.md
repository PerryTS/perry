# Networking

Compose stacks join one or more user-defined networks. Each container
spec lists the networks it joins; the engine creates the networks (if
they don't already exist) before starting any service. This page
covers the day-to-day networking patterns Perry users hit.

## Defining networks

```typescript,no-test
const stack = await up({
  version: "3.8",
  services: {
    api: { image: "myapp/api", networks: ["app-net"] },
    db:  { image: "postgres:16-alpine", networks: ["app-net"] },
  },
  networks: {
    "app-net": { driver: "bridge" },
  },
});
```

Recognised `ComposeNetwork` fields:

| Field | Type | Effect |
|---|---|---|
| `driver` | `string` | Network driver (`"bridge"` is the default; `"overlay"` for swarm). |
| `external` | `boolean` | Don't create — assume the network already exists. |
| `name` | `string` | Override the network's runtime name. |
| `internal` | `boolean` | **Internal-only**: containers attached have no external bridge or routing. See below. |
| `driver_opts` | `Record<string, string>` | Driver-specific options. |
| `labels` | `Record<string, string>` | Network labels. |

## Internal-only networks (`internal: true`)

A network with `internal: true` blocks egress to anything outside the
network. Containers on it can talk to each other, but **cannot reach the
host or the public internet**, and the host cannot reach them via
published ports. This is the canonical "private database side-channel"
pattern:

```typescript,no-test
networks: {
  "app-db-net":  { driver: "bridge", internal: true },  // db <-> api only
  "app-web-net": { driver: "bridge" },                  // api <-> host
},
services: {
  db: {
    image: "postgres:16-alpine",
    networks: ["app-db-net"],   // db is reachable ONLY from app-db-net
    // no `ports:` — postgres is unpublished
  },
  api: {
    image: "myapp/api",
    networks: ["app-db-net", "app-web-net"],
    ports: ["8080:8080"],       // api published on the host
  },
},
```

The api container straddles both networks: it can reach `db` over
`app-db-net` and accept inbound HTTP from the host on `app-web-net`.
postgres is invisible to anything not on `app-db-net`.

## Cross-service DNS

Within a user-defined bridge network, docker's embedded DNS resolves
container names to IP addresses. So if a service's `container_name` is
`forgejo-db`, sibling containers on the same network can connect to it
as `forgejo-db:5432`.

> ⚠️ **Important:** Perry's compose engine generates per-service
> container names of the form `{md5(image)[0..8]}-{random_hex8}` by
> default. It does **not** (yet) register the service KEY (`db`, `api`,
> …) as a network alias the way `docker compose` does. So a config
> like:
>
> ```typescript,no-test
> api: {
>   image: "myapp/api",
>   environment: {
>     DATABASE_URL: "postgres://user:pw@db:5432/app",  // ❌ "db" doesn't resolve
>   },
> }
> ```
>
> will fail at runtime with `dial tcp: lookup db on 127.0.0.11:53: no
> such host`. **Until service-key network aliasing lands, set
> `container_name` explicitly** and use those names in sibling URLs:

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:container-name-dns}}
```

The Forgejo example uses this pattern (`container_name: 'forgejo-db'` +
`FORGEJO__database__HOST: 'forgejo-db:5432'`). It's a documented
workaround that keeps user code idiomatic; replacing
`container_name` with service-key alias registration is a planned
runtime change that will not require any user-facing API change.

## Port mapping

Inside a service spec, `ports: ["host:container[:proto]"]` publishes
ports to the host. Examples:

| Spec | Behavior |
|---|---|
| `"8080:80"` | Host port 8080 → container port 80 (TCP). |
| `"8080:80/udp"` | Host port 8080 → container port 80 (UDP). |
| `"127.0.0.1:8080:80"` | Bind only to loopback on the host (don't expose to other LAN hosts). |
| `"3000-3010:3000-3010"` | Range mapping (UDP/TCP, host:container both inclusive). |

For services that should never be host-published (private databases,
internal-only side-cars), simply **don't list any ports**. Combined
with `internal: true` on the network, those services are unreachable
from the host even if a port slipped into the spec by mistake.

## Single-network shorthand

When every service joins the same network, you can put `networks:
['<name>']` on each service and `networks: { <name>: {...} }` once at
the root. The engine deduplicates network creation across services.

## Networks created in this session vs. external

Perry tracks **session networks** (created during this `up()` call) and
distinguishes them from `external: true` networks (assumed pre-existing
and shared across stacks). On `down()`, only session networks are
torn down — external networks are left alone, matching docker-compose
semantics.

```typescript,no-test
networks: {
  // Session: created if missing; removed on down()
  "app-net": { driver: "bridge" },

  // External: must already exist; never touched on down()
  "shared-public-net": { external: true, name: "external_pub_v1" },
},
```

## Network options for production

Common per-network knobs you'll want for production:

| Pattern | Spec |
|---|---|
| **Disable masquerade / NAT** (host-side) | `driver_opts: { "com.docker.network.bridge.enable_ip_masquerade": "false" }` |
| **Custom MTU** (matches host network) | `driver_opts: { "com.docker.network.driver.mtu": "1450" }` |
| **Stable bridge name** (for iptables rules) | `driver_opts: { "com.docker.network.bridge.name": "br-myapp" }` |
| **Tag for monitoring** | `labels: { team: "platform", environment: "prod" }` |

## See also

- [Compose orchestration](./compose.md) — full `up()` / `down()`
  reference.
- [Production patterns](./production-patterns.md) — Forgejo example
  uses the internal-db-net + public-web-net split.
- [Volumes](./volumes.md) — companion concept: networks without
  volumes is rare in production stacks.
