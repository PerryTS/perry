# Single-Container Lifecycle (`perry/container`)

`perry/container` exposes the OCI primitives that operate on **one
container at a time**: create, start, run, stop, remove, exec, logs,
inspect, plus image management. For multi-service stacks, see
[`perry/compose`](./compose.md) — but you can mix the two modules in the
same program (a long-running compose stack plus one-off `run()` helpers
against it is a normal pattern).

Every async function returns a `Promise`. The runtime backend (docker,
podman, apple/container, …) is auto-detected on first use; see
[Overview](./overview.md#backend-auto-detection) for the probe order
and override knobs.

## Running a container

`run()` creates and starts a container in one shot, returning a handle:

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:run-simple}}
```

The full `ContainerSpec` accepts:

| Field | Type | Effect |
|---|---|---|
| `image` | `string` | (required) Image reference, e.g. `"alpine:3.19"`. |
| `name` | `string` | Explicit container name. Defaults to `{md5(image)[0..8]}-{random_hex8}` when unset. |
| `cmd` | `string[]` | Command-line override (overrides the image's CMD). |
| `entrypoint` | `string[]` | Entrypoint override. |
| `env` | `Record<string, string>` | Environment variables. |
| `ports` | `string[]` | Port maps in `"host:container"` form, e.g. `["8080:80"]`. |
| `volumes` | `string[]` | Volume mounts in `"host:container[:ro]"` form, e.g. `["./data:/data:ro"]`. |
| `network` | `string` | Network name to attach to. |
| `rm` | `boolean` | Auto-remove on exit (`docker run --rm`). |
| `labels` | `Record<string, string>` | Container labels. |
| `read_only` | `boolean` | Mount the root filesystem read-only. |
| `privileged` | `boolean` | Run privileged. **Use sparingly.** |
| `user` | `string` | UID, username, or `"UID:GID"`. |
| `workdir` | `string` | Working directory inside the container. |
| `cap_add` | `string[]` | Linux capabilities to add (e.g. `["NET_BIND_SERVICE"]`). |
| `cap_drop` | `string[]` | Linux capabilities to drop (e.g. `["ALL"]`). |
| `seccomp` | `string` | Seccomp profile path or `"default"` (the runtime's default profile). |
| `no_new_privileges` | `boolean` | Sets `--security-opt no-new-privileges` — SUID/SGID binaries inside the container can't gain privileges via execve. |

When `seccomp` or `no_new_privileges` is set, `run()`/`create()`
automatically route through the engine's security-aware launch path
(the same one `perry/compose` uses), which also normalizes the spec
against the detected backend's capabilities — on apple/container the
two flags have no equivalent and are dropped with a warning instead of
crashing the CLI. See [Security](./security.md) for the security knobs
in depth.

### Hardened single-container run

For an untrusted workload (e.g. running user-supplied code, executing a
build script from an untrusted source) the recommended starting point
is "drop everything, add back what you need":

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:run-secure}}
```

## Inspect, list, logs, exec

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:list-inspect}}
```

| Function | Signature | Notes |
|---|---|---|
| `list(all?)` | `(all: boolean) → Promise<ContainerInfo[]>` | `all=true` includes stopped containers. |
| `inspect(id)` | `(id: string) → Promise<ContainerInfo>` | Throws if the container doesn't exist. |
| `logs(id, opts?)` | `(id, { tail?: number }) → Promise<ContainerLogs>` | Returns a registry handle to a `{ stdout, stderr }` pair. |
| `exec(id, cmd, opts?)` | `(id, cmd[], { env?, workdir? })` | Runs a command in the container. Returns a `ContainerLogs` handle. |
| `stop(id, timeout?)` | `(id, seconds: number)` | Sends SIGTERM, then SIGKILL after `timeout` seconds. |
| `start(id)` | `(id)` | Re-starts a stopped container. |
| `remove(id, force?)` | `(id, force: boolean)` | `force=true` is `docker rm -f`. |

> **Note on the `logs` and `exec` return shape:** today the FFI returns
> a registry-id handle into a `Vec<ContainerLogs>` rather than a JS
> object. Treat the returned value as opaque — a future ergonomics task
> will expose `.stdout` / `.stderr` directly on the JS side. The
> `ContainerLogs` shape over the wire is `{ stdout: string, stderr:
> string }`.

## Image management

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:image-mgmt}}
```

| Function | Signature |
|---|---|
| `pullImage(reference)` | `(reference: string) → Promise<void>` |
| `listImages()` | `() → Promise<ImageInfo[]>` |
| `removeImage(reference, force?)` | `(reference: string, force: boolean) → Promise<void>` |

When `PERRY_CONTAINER_VERIFY_IMAGES=1` is set, every `run()`,
`create()`, and `pullImage()` call routes through cosign keyless
verification against the Chainguard identity. See
[Security → Image verification](./security.md#image-verification).

## Container naming

The default name is `{md5(image)[0..8]}-{random_hex8}` — a stable
8-character hash of the image plus a per-call random suffix. This is
fine for one-off `run()` calls but makes containers hard to find later
unless you set `name:` explicitly. **For anything you'll re-target
later (with `inspect`, `logs`, `exec`, etc.), set `name:` upfront.**

```typescript,no-test
const handle = await run({
  image: "alpine:3.19",
  name: "build-helper",   // ← stable handle
  cmd: ["sh", "-c", "echo 'hi from build-helper'"],
  rm: true,
});
```

## Backend introspection

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:backend-detect}}
```

`getBackend()` is synchronous and returns the canonical backend name
(`"docker"`, `"podman"`, `"apple/container"`, etc.). It will perform a
synchronous in-place probe on first call so the result is always the
live name; calls after the first hit a cached `OnceLock` and return
instantly.

`detectBackend()` is async and returns a JSON array of *every* probed
candidate with `{ name, available, reason, version, mode,
isolationLevel }` per entry. Use it to surface a "diagnostics" view in
your CLI / dashboard.

## See also

- [Compose orchestration](./compose.md) — multi-service stacks.
- [Networking](./networking.md) — port maps, networks, the
  cross-service DNS gotcha.
- [Security](./security.md) — capability isolation patterns.
