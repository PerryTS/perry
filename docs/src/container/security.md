# Security

Containers don't isolate themselves; you isolate them. Perry exposes the
standard OCI security knobs on both `ContainerSpec` (single-container)
and `ComposeService` (orchestrated stacks), plus first-party support
for Sigstore / cosign image verification and a workload-graph policy
tier API for declarative isolation levels.

## Per-container security knobs

The same set of fields work on `run()`, `create()`, and any service in a
compose `up()`:

| Field | Type | Effect | Cross-backend |
|---|---|---|---|
| `read_only` | `boolean` | Mount the root filesystem as read-only. Forces all writable state to be in declared volumes. | All backends |
| `privileged` | `boolean` | Run privileged: grants ALL Linux capabilities + access to host devices. **Avoid unless absolutely necessary.** | Docker / Podman / Lima only — apple/container has no concept and **drops the field** with a warning |
| `user` | `string` | UID, username, or `"UID:GID"` — runs the container's processes as that identity. The image's CMD ignores this if it does its own user-switching, but most properly-built images respect it. | All backends |
| `workdir` | `string` | Working directory inside the container. | All backends |
| `cap_add` | `string[]` | Linux capabilities to add. Specific (e.g. `["NET_BIND_SERVICE"]`), not blanket. | All backends |
| `cap_drop` | `string[]` | Capabilities to drop. `["ALL"]` is the canonical "drop everything" starting point. | All backends |
| `seccomp` | `string` | Seccomp profile path or `"default"` (uses the runtime's default profile). | Docker / Podman / Lima only — apple/container has no equivalent and **drops the field** with a warning |
| `no_new_privileges` | `boolean` | Sets `--security-opt no-new-privileges` — SUID/SGID binaries inside the container can't gain privileges via execve. | Docker / Podman / Lima only — dropped on apple/container |

On a compose service the last two are spelled in compose-spec form
instead: `security_opt: ["seccomp=<path-or-default>",
"no-new-privileges"]`. The engine parses those entries into the same
internal `SecurityProfile` that `run()`/`create()` build from the
spec-level fields, so both APIs hand the backend identical flags.

> ⚠️ **Cross-backend security caveat.** `privileged`, `seccomp`,
> `--security-opt no-new-privileges`, IPC/PID namespace sharing, and
> SELinux mount labels are **not honored on apple/container** — its
> Apple-VM model means those concepts don't translate. Perry's
> normalization pass drops the fields and emits a `tracing::warn!`
> rather than silently downgrading the security policy. For production
> deployments that demand cross-backend parity, set
> `EnforcementMode::Strict` on the engine — any unsupported security
> field becomes a hard `up()` failure rather than a silent drop. Full
> matrix at [Cross-Backend Determinism](./determinism.md).

## Recommended baseline

Start with maximum isolation and add back only what the workload needs:

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:run-secure}}
```

Field-by-field rationale:

- `read_only: true` — even an exploit that lands code execution can't
  persist to the image's filesystem. Anything mutable goes into a
  declared volume.
- `cap_drop: ["ALL"]` — removes Linux capabilities the workload didn't
  explicitly ask for. Most apps need none.
- `user: "nobody"` — non-root inside the container. If the image
  doesn't have a `nobody` user, replace with `"65534:65534"` (the
  numeric UID/GID of `nobody` on most distros).
- `workdir: "/tmp"` — the only writable location under
  `read_only: true` is `/tmp` (which is `tmpfs`-backed by default).
- `seccomp: "default"` — uses docker's default seccomp profile (~50
  syscalls blocked).

## Capability addition patterns

`cap_drop: ["ALL"]` plus targeted `cap_add`:

| Workload | Capabilities |
|---|---|
| **Web server binding to port 80/443** | `cap_add: ["NET_BIND_SERVICE"]` |
| **Network namespace manipulation** | `cap_add: ["NET_ADMIN"]` |
| **Kernel time setting** | `cap_add: ["SYS_TIME"]` |
| **chown** to other users (rare) | `cap_add: ["CHOWN"]` |
| **Bind-mount filesystems inside** | `cap_add: ["SYS_ADMIN"]` (still avoid if possible) |

The full capability list is in `man capabilities(7)`. Always start with
`cap_drop: ["ALL"]` and add only what fails when removed — most
applications need zero capabilities.

## Image verification

Set `PERRY_CONTAINER_VERIFY_IMAGES=1` to enable cosign keyless
verification on every `run()`, `create()`, and `pullImage()` call:

```bash
export PERRY_CONTAINER_VERIFY_IMAGES=1
./my-app
```

Perry's verifier:

1. Resolves the image tag to its digest via `inspect_image`.
2. Looks up the digest in an in-memory `VERIFICATION_CACHE` —
   subsequent runs against the same digest are free.
3. Runs `cosign verify --certificate-identity ${CHAINGUARD_IDENTITY}
   --certificate-oidc-issuer ${CHAINGUARD_ISSUER} <ref>@<digest>` and
   caches pass/fail.
4. On fail, the FFI rejects with a `verification failed` error
   (the container is never created).

Default identity / issuer point at Chainguard's keyless signing flow:

| Const | Value |
|---|---|
| `CHAINGUARD_IDENTITY` | `https://github.com/chainguard-images/images/.github/workflows/sign.yaml@refs/heads/main` |
| `CHAINGUARD_ISSUER` | `https://token.actions.githubusercontent.com` |

For your own org's images, override these via the (planned) per-call
verification options. For now, using Chainguard-signed base images is
the path of least resistance — `cgr.dev/chainguard/<tool>` is signed.

> **Cosign required.** Set `PERRY_CONTAINER_VERIFY_IMAGES=1` only when
> `cosign` is installed and on `PATH`. The verification is OFF by
> default so the bare-metal `./my-app` execution doesn't depend on a
> separate cosign install.

## Capability sandbox helper

For one-off command execution against an untrusted image (CI helper,
build tool, code-evaluation sandbox), use the
[`run_capability` pattern](./containers.md#hardened-single-container-run)
which wraps `run()` with the maximum-isolation defaults:

- `read_only: true`
- `cap_drop: ["ALL"]`
- No network attached
- `user: "nobody"`
- Image verified via cosign before pull

This is the same path the internal `perry-stdlib::container::capability`
module uses for shell-command sandboxing in plugin systems.

## Workload-graph policy tiers (`perry/workloads`)

For multi-node deployments where different workloads have different
trust levels, the workload-graph engine accepts a per-node `policy`:

```typescript,no-test
import { graph, runGraph, runtime, policy } from "perry/workloads";

const g = graph("my-app", {
  trusted_db:    { image: "postgres:16-alpine",
                   runtime: runtime.oci(),
                   policy:  policy.default() },        // no extra hardening

  isolated_api:  { image: "myapp/api",
                   runtime: runtime.oci(),
                   policy:  policy.isolated() },       // no_network=true

  hardened_proxy: { image: "myapp/proxy",
                    runtime: runtime.oci(),
                    policy:  policy.hardened() },      // read_only_root + seccomp

  untrusted_eval: { image: "myapp/sandbox",
                    runtime: runtime.microvm(),         // ← required by tier
                    policy:  policy.untrusted() },     // microVM-only, all hardening on
});

await runGraph(g);
```

The four `PolicyTier` levels and what they enforce:

| Tier | `no_network` | `read_only_root` | `seccomp` | `microvm` |
|---|---|---|---|---|
| `default()` | — | — | — | — |
| `isolated()` | ✅ | — | — | — |
| `hardened()` | — | ✅ | ✅ | — |
| `untrusted()` | ✅ | ✅ | ✅ | **required** |

`untrusted` requires kernel-level isolation (i.e. a microVM, not a
shared-kernel container). When the active backend doesn't expose a
microVM runtime (`apple/container`'s VM mode, Lima, Firecracker), the
engine returns `BackendNotAvailable` rather than silently dropping the
isolation guarantee. Use `PERRY_ALLOW_UNTRUSTED_SHARED_KERNEL=1` to opt
out — **not recommended for actually-untrusted code.**

User-explicit per-flag overrides on top of a tier are honored: setting
`policy.tier = "default"` and `no_network: true` produces an
isolated-network default-tier node.

## Defense in depth

Stacking patterns for production:

1. **Verify images** (`PERRY_CONTAINER_VERIFY_IMAGES=1`).
2. **Run as non-root** (`user: "nobody"` or numeric UID).
3. **Drop all capabilities, add specific ones back** (`cap_drop:
   ["ALL"]` + minimal `cap_add`).
4. **Read-only root filesystem** (`read_only: true`).
5. **Internal networks for the database side** (`internal: true` on the
   db's network — see [Networking](./networking.md#internal-only-networks-internal-true)).
6. **No published ports for private services** (omit `ports:` on
   internal-only services).
7. **Resource limits** (planned: `mem_limit`, `cpu_limit` on Service).

## See also

- [Compose orchestration](./compose.md) — applying these knobs in a
  stack spec.
- [Production patterns](./production-patterns.md) — Forgejo example
  uses several of these (internal-only db net, published web port,
  USER_UID/GID).
- [Networking](./networking.md) — internal-only networks for
  database isolation.
