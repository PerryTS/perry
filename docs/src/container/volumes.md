# Volumes

Container filesystems are ephemeral by default — once a container is
removed, anything written to its layers is gone. Production deployments
need volumes for the data that should survive container restarts +
upgrades: database storage, uploaded files, generated config, etc.

Perry supports the three Compose-spec volume modes:

| Mode | Spec example | Use case |
|---|---|---|
| **Named volume** | `["app-pgdata:/var/lib/postgresql/data"]` | Database state, durable per-app data. |
| **Bind mount** | `["./config:/app/config:ro"]` | Host-supplied config or secrets. |
| **System pass-through** | `["/etc/timezone:/etc/timezone:ro"]` | Read-only access to host system files. |

## Declaring named volumes

Named volumes must be declared at the spec root and referenced by name
in each service's `volumes` array:

```typescript,no-test
const stack = await up({
  services: {
    db: {
      image: "postgres:16-alpine",
      volumes: ["app-pgdata:/var/lib/postgresql/data"],
    },
  },
  volumes: {
    "app-pgdata": { driver: "local" },
  },
});
```

Recognised `ComposeVolume` fields:

| Field | Type | Effect |
|---|---|---|
| `driver` | `string` | Volume driver (`"local"` is the default). |
| `external` | `boolean` | Don't create — assume the volume already exists. |
| `name` | `string` | Override the volume's runtime name. |

## Bind mounts

For host-supplied data, use the `host:container[:options]` form:

```typescript,no-test
volumes: [
  "./config:/app/config:ro",     // read-only config dir from host
  "/var/log/myapp:/app/logs",    // bidirectional logs
],
```

Permissions are governed by the host filesystem and the container's
running UID. If the container runs as a non-root user (as it should —
see [Security](./security.md)), make sure the host directory is owned
by a matching UID, **or** explicitly set the container UID via
`USER_UID` / `USER_GID` env vars in the image (the Forgejo image does
this).

## System pass-throughs

Read-only mounts of host system files are common for time / DNS /
locale alignment:

```typescript,no-test
volumes: [
  "/etc/timezone:/etc/timezone:ro",
  "/etc/localtime:/etc/localtime:ro",
],
```

Best-effort: hosts where the source path doesn't exist (e.g. some
minimal Alpine VMs) just see a missing mount source — docker tolerates
it; the container falls back to UTC / system defaults.

## Preservation on `down()`

By default, **`down(handle)` preserves named volumes**:

```typescript,no-test
await down(stack);                       // containers + networks gone, volumes survive
await down(stack, { volumes: false });   // same — explicit preserve
await down(stack, { volumes: true });    // ⚠ volumes ALSO removed (DESTROYS DATA)
```

This matches `docker compose down` semantics:

| Command | Containers | Networks | Volumes |
|---|---|---|---|
| `down(handle)` | removed | removed | **kept** |
| `down(handle, { volumes: true })` | removed | removed | **removed** |

After a `down(handle)`, you can `up(spec)` again with the same volume
declarations and the database / file state from before is still there.
That's how the [Forgejo example](./production-patterns.md) supports
"deploy → tear-down → redeploy" cycles without data loss.

> ⚠️ **Forgejo / Postgres redeploy gotcha:** if you used randomly
> generated passwords or secret keys on the first deploy, **the next
> redeploy with new random secrets will fail** because postgres
> authenticates against the old password and Forgejo can't decrypt
> the existing config dir with a different SECRET_KEY. For
> redeploys against the same volumes, set
> `FORGEJO_DB_PASSWORD` / `FORGEJO_SECRET_KEY` /
> `FORGEJO_INTERNAL_TOKEN` to **stable** values (e.g. via an `.env`
> file). The Forgejo example's doc-comment has the canonical pattern.

## External volumes

Mark a volume `external: true` to share it across stacks or to use a
volume created by a different process (e.g. `docker volume create
team-shared-cache` ahead of time):

```typescript,no-test
volumes: {
  "shared-cache": { external: true, name: "team-shared-cache" },
},
```

External volumes are **never removed** by `down(handle, { volumes: true
})` — that flag only drops volumes the engine itself created. This
matches docker-compose semantics; if you want the external volume gone,
remove it explicitly with `docker volume rm team-shared-cache`.

## Volume naming and ownership

Perry project-scopes volume (and network) names as `<project>_<name>`
unless the volume is `external: true` or carries an explicit `name:`
override — so `forgejo-pgdata` under project `forgejo` becomes the
docker volume `forgejo_forgejo-pgdata`. The project identifier comes
from `ComposeSpec.name` and defaults to `"perry-stack"` when omitted —
so two stacks that *both* leave `name` unset and declare the same
volume key still collide. Give each stack a distinct project name for
multi-stack isolation:

```typescript,no-test
await up({
  name: "myapp-staging",              // → volume myapp-staging_pgdata
  services: { /* … */ },
  volumes: {
    pgdata: { driver: "local" },
  },
});
```

## Inspecting volume state

The `perry/container` and `perry/compose` modules don't expose a JS
`inspectVolume()` helper today — for now, inspect with the underlying
runtime CLI:

```bash
docker volume ls --filter name=app-       # list app-prefixed volumes
docker volume inspect app-pgdata          # mountpoint, driver, labels
docker run --rm -v app-pgdata:/data \      # mount + inspect contents
  alpine ls -la /data
```

## Backup patterns

The standard "tar the volume into the host" backup recipe:

```bash
docker run --rm -v app-pgdata:/data:ro -v $(pwd):/backup alpine \
  tar czf /backup/pgdata-$(date +%F).tar.gz -C /data .
```

For a pure-Perry approach, drive that with `perry/container.run()`:

```typescript,no-test
await run({
  image: "alpine:3.19",
  cmd: ["sh", "-c",
    "tar czf /backup/pgdata-$(date +%F).tar.gz -C /data ."],
  volumes: [
    "app-pgdata:/data:ro",
    "./backups:/backup",
  ],
  rm: true,
});
```

## See also

- [Compose orchestration](./compose.md) — `down(handle, opts)` reference.
- [Production patterns](./production-patterns.md) — Forgejo example
  uses three named volumes (pgdata, data, config).
- [Security](./security.md) — read-only mounts and ownership patterns.
