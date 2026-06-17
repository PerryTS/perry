# Production Patterns

This page is a guided tour of [`example-code/forgejo-deployment`](https://github.com/PerryTS/perry/tree/main/example-code/forgejo-deployment),
a working production-quality deployment of [Forgejo](https://forgejo.org/)
(self-hosted Git) using the real Forgejo image from the official
`data.forgejo.org` registry. The example was driven end-to-end against
live Docker; the patterns here are what survived.

The full source is at [`example-code/forgejo-deployment/main.ts`](https://github.com/PerryTS/perry/tree/main/example-code/forgejo-deployment/main.ts).
This page documents the *patterns*, not every line.

## Lifecycle: `up + verify + exit 0` then a separate `--down`

Perry's runtime currently does not deliver `process.on('SIGINT', ...)`
to your TS code. So the canonical "Ctrl-C tears down the stack" pattern
isn't writable today. Instead, follow the `docker compose up -d` /
`docker compose down` model: deploy + verify + exit 0, with teardown
behind a separate `--down` invocation:

```typescript,no-test
async function main() {
  const args = process.argv.slice(2);
  const config = buildConfig();
  if (args.includes("--down")) {
    await cmdDown(config);
  } else {
    await cmdUp(config);
  }
}
```

The example's `cmdUp`:

1. Pre-flight backend probe + port-conflict guard.
2. Call `up()` with the canonical spec.
3. Poll readiness probes (postgres `pg_isready`, then forgejo
   `/api/healthz`).
4. Print an operator-facing banner with URLs + "how to tear down".
5. Exit 0. Containers keep running thanks to `restart:
   unless-stopped`.

The example's `cmdDown`:

1. Re-call `up()` with the same spec — idempotent: services already
   running are detected and skipped, returning the same handle the
   original deploy got.
2. Call `down(handle, { volumes: destroy })`. `destroy` is set from
   `FORGEJO_DESTROY_ON_EXIT=1`.

## Two-network split: internal db + public web

The Forgejo example puts postgres on an internal-only network and
forgejo on both that network and a public bridge:

```typescript,no-test
networks: {
  "forgejo-db-net":  { driver: "bridge", internal: true }, // postgres unreachable from host
  "forgejo-web-net": { driver: "bridge" },                 // forgejo's web + SSH ports
},
services: {
  db: {
    networks: ["forgejo-db-net"],
    // no `ports:` — postgres is invisible to the host
  },
  forgejo: {
    networks: ["forgejo-db-net", "forgejo-web-net"],
    ports: ["3000:3000", "2222:22"],  // public web + SSH
  },
},
```

Why: postgres should never be reachable from the host (or from sibling
stacks), but forgejo needs both inbound HTTP from the host AND outbound
DB queries to postgres. Two networks is the cleanest expression of
that split.

## Stable container names for cross-service DNS

Perry's compose engine creates each container with a `{md5}-{random}`
derived name and doesn't yet register the service KEY (`db`,
`forgejo`) as a network alias. So
`FORGEJO__database__HOST: 'db:5432'` would fail name resolution at
runtime. The Forgejo example pins explicit `container_name` values:

```typescript,no-test
const dbHostname      = "forgejo-db";
const forgejoHostname = "forgejo-app";

services: {
  db: {
    image: `postgres:${pgVersion}`,
    container_name: dbHostname,                  // ← stable target
    // …
  },
  forgejo: {
    image: `data.forgejo.org/forgejo/forgejo:${version}`,
    container_name: forgejoHostname,
    environment: {
      FORGEJO__database__HOST: `${dbHostname}:5432`,  // ← refers to it
      // …
    },
  },
},
```

See [Networking → Cross-service DNS](./networking.md#cross-service-dns)
for the full backstory and why this is the workaround until
service-key network-alias support lands.

## OpenSSH on :22 + `START_SSH_SERVER=false`

Forgejo's official image runs `/usr/sbin/sshd` on container port 22 in
its entrypoint script, then runs the forgejo binary. If you also set
`FORGEJO__server__START_SSH_SERVER=true`, forgejo's Go-based built-in
SSH server tries to bind :22 too — and the container exit-0's with
"bind: address already in use".

The standard Forgejo deployment pattern is to **let OpenSSH handle SSH
on :22 and tell forgejo not to start its own**:

```typescript,no-test
environment: {
  FORGEJO__server__START_SSH_SERVER: "false",   // ← critical
  FORGEJO__server__SSH_PORT:         "2222",    // public host port
  FORGEJO__server__SSH_LISTEN_PORT:  "22",      // container-internal port
  // …
},
```

Forgejo writes git users' authorized_keys to `/data/git/.ssh/`, which
the in-container OpenSSH consumes. Git operations route through sshd on
:22, then forgejo's `gitea-shell` script.

## Healthcheck-gated dependency startup

postgres takes ~5–10 seconds to initialise on first run (initdb +
listener bind). Without gating, forgejo starts immediately, can't
connect, and burns retry budget. The fix is a per-service
`healthcheck` plus `depends_on: { svc: { condition: 'service_healthy'
} }`:

```typescript,no-test
db: {
  image: "postgres:16-alpine",
  // …
  healthcheck: {
    test: ["CMD-SHELL", "pg_isready -U forgejo -d forgejo"],
    interval: "5s",
    timeout: "3s",
    retries: 10,
    start_period: "30s",
  },
},
forgejo: {
  // …
  depends_on: { db: { condition: "service_healthy" } },
},
```

Even with that, the example *also* runs an explicit readiness loop
post-`up()` for the full HTTP `/api/healthz` path — the healthcheck
gates **container startup** but the operator banner shouldn't print
until the API is *serving*:

```typescript,no-test
async function waitForForgejo(stack: number, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      // Probe from INSIDE the forgejo container so the docker-proxy
      // bind-up window doesn't trip the host-side curl.
      await exec(stack, "forgejo", [
        "wget", "-q", "-O", "/dev/null",
        "--timeout=2", "--tries=1",
        "http://127.0.0.1:3000/api/healthz",
      ]);
      return true;
    } catch (_e) {
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
  return false;
}
```

`/api/healthz` is Forgejo's no-auth liveness endpoint that returns 200
once the web server is up AND the database / cache subsystems pinged
successfully. Don't use `/api/v1/version` — when
`REQUIRE_SIGNIN_VIEW=true` (a production-hardening default) it returns
401, and `wget` exits non-zero on HTTP error responses.

## Stable secrets for redeploy

The Forgejo example's `buildConfig()` uses **truthy-fallback** semantics
for env vars (`process.env[name] || fallback`) because Perry's
`process.env[NONEXISTENT]` returns an empty-ish value where strict
equality to `undefined` / `''` doesn't hold:

```typescript,no-test
function envOr(name: string, fallback: string): string {
  return (process.env[name] as string | undefined) || fallback;
}
```

The defaults for the three secret-bearing fields are random hex:

```typescript,no-test
dbPassword:      envOr('FORGEJO_DB_PASSWORD', randomHex(32)),
secretKey:       envOr('FORGEJO_SECRET_KEY',     randomHex(32)),
internalT:       envOr('FORGEJO_INTERNAL_TOKEN', randomHex(52)),
```

This is fine for **first-run** / dev / smoke-test, but **breaks any
subsequent run against the same volumes** because:

- Postgres rows were authored under the prior password — new password
  rejects the connection.
- Forgejo's `/data/gitea/conf/app.ini` is encrypted with the prior
  `SECRET_KEY` — Forgejo can't decrypt it on startup.

For production, **set them to stable values** via an `.env` file or a
secrets manager:

```bash
# .env
FORGEJO_DB_PASSWORD=$(openssl rand -hex 32)
FORGEJO_SECRET_KEY=$(openssl rand -hex 32)
FORGEJO_INTERNAL_TOKEN=$(openssl rand -hex 52)

# deploy.sh
source .env
./forgejo_app
```

Generate once, store in a secrets manager, redeploy as many times as
needed against the same volumes.

## First-run admin user

Forgejo's installer is locked (`INSTALL_LOCK=true`) so the GUI
installer doesn't run on first request. To create the initial admin
user, exec the `forgejo admin user create` CLI inside the container:

```bash
docker exec forgejo-app forgejo admin user create \
  --admin --username root --email root@example.com \
  --random-password
```

The `--random-password` flag prints the generated password to stdout
once — capture it from the docker logs and store it somewhere safe.

## Idempotent redeploy

Running `./forgejo_app` a second time on a healthy stack is a no-op:
`up()` calls `inspect` on each service, sees `running`, and skips. The
operator banner prints immediately and the readiness loops exit fast
because the services are already serving. This is by design — it's
the same property `docker compose up -d` has.

For a "rip and replace" upgrade (new image tag, new env values that
require recreate), do an explicit `--down` first:

```bash
./forgejo_app --down                        # preserve volumes
FORGEJO_VERSION=12 ./forgejo_app            # redeploy with new version
```

The volumes carry forward automatically; `up()` detects the existing
`forgejo-data` and `forgejo-pgdata` volumes via `inspect_volume` and
attaches them to the new containers without re-creating.

## Running it

```bash
# Build perry once
cargo build --release -p perry-runtime -p perry-stdlib -p perry

# Build the example
cd example-code/forgejo-deployment
../../target/release/perry compile main.ts -o forgejo_app

# Deploy
./forgejo_app
# 🔧 Backend: docker
# 🚀 Deploying Forgejo 11 (data.forgejo.org/forgejo/forgejo:11)
# …
# 🎉  Forgejo 11 is up and ready.

# Visit http://localhost:3000/ in a browser.

# Tear down (preserves volumes for redeploy):
./forgejo_app --down

# Tear down + drop volumes (DESTROYS DATA):
FORGEJO_DESTROY_ON_EXIT=1 ./forgejo_app --down
```

## See also

- [Compose orchestration](./compose.md) — `up()` / `down()` reference.
- [Networking](./networking.md) — the internal-net + public-net split.
- [Volumes](./volumes.md) — preservation across `down()`.
- [Security](./security.md) — capability hardening + image
  verification.
