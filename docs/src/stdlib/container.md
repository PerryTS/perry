# Containers

The `perry/container` and `perry/compose` modules manage OCI containers
and multi-container stacks directly from Perry programs — same model as
`docker compose up`, but with the spec as a TS object literal and the
orchestration engine running natively in-process (no shell-out to
`docker-compose`).

For the full container subsystem documentation see the dedicated
**Containers** section:

- **[Overview](../container/overview.md)** — module layout, backend
  auto-detection, and the canonical lifecycle pattern.
- **[Single-Container Lifecycle](../container/containers.md)** —
  `perry/container`: `run`, `inspect`, `logs`, `exec`, image management.
- **[Compose Orchestration](../container/compose.md)** —
  `perry/compose`: `up`, `down`, `ps`, healthcheck-gated `depends_on`,
  env-var interpolation.
- **[Networking](../container/networking.md)** — internal-only
  networks, port maps, and the cross-service-DNS workaround.
- **[Volumes](../container/volumes.md)** — named vs. bind mounts and
  preservation semantics on `down()`.
- **[Security](../container/security.md)** — capability isolation,
  cosign image verification, workload-graph policy tiers.
- **[Production Patterns](../container/production-patterns.md)** —
  full Forgejo deployment case study with the patterns it surfaced.

## Quick start

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:compose-up-simple}}
```

```typescript
{{#include ../../examples/stdlib/container/snippets.ts:compose-down}}
```

See the linked pages above for the full API surface, production
patterns, and case studies.
