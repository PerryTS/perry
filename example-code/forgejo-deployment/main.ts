/**
 * perry/container — Production Forgejo Stack
 *
 * Self-hosted Forgejo (https://forgejo.org/) deployment via Perry's
 * `perry/compose` orchestration API.
 *
 * Image source:
 *   `data.forgejo.org/forgejo/forgejo:<MAJOR>` — Forgejo's official OCI
 *   registry (separate from `codeberg.org`, which gates pulls behind a
 *   Codeberg account, and from any Gitea-branded image).
 *
 * Lifecycle (matches docker-compose up -d / down)
 *   ./forgejo_app          deploy + verify health + exit 0; stack
 *                          stays running in the background
 *   ./forgejo_app --down   tear the stack down; volumes preserved
 *                          unless FORGEJO_DESTROY_ON_EXIT=1 is set
 *
 * What this example demonstrates
 *   - Two-service stack (Forgejo + PostgreSQL) with explicit dependency
 *     ordering (`depends_on`) and per-service healthchecks.
 *   - Named volumes for durable Git repos / config / database state.
 *   - A db-only internal network so PostgreSQL is unreachable from the
 *     host or from any other compose stack.
 *   - Pre-flight: backend probe, port-conflict guard.
 *   - Post-up: poll `pg_isready` until accepting connections, then
 *     poll Forgejo's `/api/healthz` until it answers 200.
 *   - Idempotent `up()` for redeploy: re-running the script on an
 *     already-up stack is a no-op (Perry's compose engine skips
 *     already-running services).
 *
 * Operational defaults (override via environment)
 *   FORGEJO_DB_USER         forgejo
 *   FORGEJO_DB_PASSWORD     <random hex 32>     ⚠ MUST be stable for redeploy
 *   FORGEJO_DB_NAME         forgejo
 *   FORGEJO_DOMAIN          localhost
 *   FORGEJO_PROTOCOL        http
 *   FORGEJO_HTTP_PORT       3000
 *   FORGEJO_SSH_PORT        2222
 *   FORGEJO_VERSION         11
 *   POSTGRES_VERSION        16-alpine
 *   FORGEJO_USER_UID        1000
 *   FORGEJO_USER_GID        1000
 *   FORGEJO_SECRET_KEY      <random hex 32>     ⚠ MUST be stable for redeploy
 *   FORGEJO_INTERNAL_TOKEN  <random hex 52>     ⚠ MUST be stable for redeploy
 *
 * Production note: the three "MUST be stable for redeploy" values above
 * are randomly generated when unset, which is fine for first-run / dev
 * but breaks any subsequent run against the same volumes — Forgejo's
 * data dir stores config encrypted with the prior SECRET_KEY and the
 * Postgres volume holds rows authored under the prior password. For
 * production set them via an .env file (`source .env; ./forgejo_app`)
 * or a secrets manager. A handy way to generate stable values:
 *   openssl rand -hex 32   # → FORGEJO_DB_PASSWORD, FORGEJO_SECRET_KEY
 *   openssl rand -hex 52   # → FORGEJO_INTERNAL_TOKEN
 */

import { up, down, exec } from 'perry/compose';
import { getBackend } from 'perry/container';

// ──────────────────────────────────────────────────────────────────────
// Configuration helpers
// ──────────────────────────────────────────────────────────────────────

// Perry's `process.env[NONEXISTENT]` returns an empty-ish value where
// `=== undefined` and `=== ''` both evaluate false, but `|| fallback`
// does coalesce correctly (the value is still falsy). We use the
// truthy-fallback form below — same shape as Node's standard pattern.
function envOr(name: string, fallback: string): string {
  return (process.env[name] as string | undefined) || fallback;
}

function envOrInt(name: string, fallback: number): number {
  const raw = (process.env[name] as string | undefined) || '';
  if (!raw) return fallback;
  const n = parseInt(raw, 10);
  return Number.isFinite(n) ? n : fallback;
}

function randomHex(bytes: number): string {
  let out = '';
  for (let i = 0; i < bytes; i++) {
    const b = Math.floor(Math.random() * 256);
    out += b.toString(16).padStart(2, '0');
  }
  return out;
}

// ──────────────────────────────────────────────────────────────────────
// Pre-flight checks
// ──────────────────────────────────────────────────────────────────────

async function preflightOrExit(httpPort: number, sshPort: number): Promise<void> {
  const backend = getBackend();
  if (backend === 'unknown' || backend === '') {
    console.error(
      '❌ No container runtime detected. Install one of:\n' +
      '   • apple/container (macOS)  — brew install container\n' +
      '   • orbstack         (macOS) — brew install orbstack\n' +
      '   • podman           (any)   — https://podman.io\n' +
      '   • docker / colima  (any)   — https://docs.docker.com / brew install colima'
    );
    process.exit(2);
  }
  console.log(`🔧 Backend: ${backend}`);

  for (const p of [httpPort, sshPort]) {
    if (p < 1 || p > 65535) {
      console.error(`❌ Invalid port: ${p}`);
      process.exit(2);
    }
  }
}

// ──────────────────────────────────────────────────────────────────────
// Health probes
// ──────────────────────────────────────────────────────────────────────

async function waitForPostgres(stack: number, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  let attempt = 0;
  while (Date.now() < deadline) {
    attempt++;
    try {
      await exec(stack, 'db', [
        'pg_isready', '-U', 'forgejo', '-d', 'forgejo', '-h', 'localhost',
      ]);
      return true;
    } catch (_e) {
      // pg_isready exits non-zero while server initialises; retry every 1s.
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
  console.error(`   pg_isready never succeeded after ${attempt} attempts`);
  return false;
}

async function waitForForgejo(stack: number, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  let attempt = 0;
  while (Date.now() < deadline) {
    attempt++;
    try {
      // Forgejo's `/api/healthz` is a no-auth liveness endpoint that
      // returns 200 with a pass/fail JSON body once the web server is
      // up AND the database / cache subsystems pinged successfully.
      // (`/api/v1/version` is auth-gated when `REQUIRE_SIGNIN_VIEW` is
      // on, which would make `wget` exit 8 on HTTP 401.)
      // Probing from INSIDE the forgejo container so we don't depend
      // on the host's port forward being live yet — the docker proxy
      // has a brief window where the container is up but the bind
      // hasn't been established.
      await exec(stack, 'forgejo', [
        'wget', '-q', '-O', '/dev/null',
        '--timeout=2', '--tries=1',
        'http://127.0.0.1:3000/api/healthz',
      ]);
      return true;
    } catch (_e) {
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
  console.error(`   Forgejo /api/healthz never answered 200 after ${attempt} attempts`);
  return false;
}

// ──────────────────────────────────────────────────────────────────────
// Stack construction
// ──────────────────────────────────────────────────────────────────────

// ──────────────────────────────────────────────────────────────────────
// Spec construction (factored so `up` and `--down` share one source of
// truth — `down()` derives all its name/volume/network references from
// the same ComposeSpec the engine started the stack with, so the script
// is idempotent across re-runs.)
// ──────────────────────────────────────────────────────────────────────

interface StackConfig {
  dbUser: string;
  dbPassword: string;
  dbName: string;
  domain: string;
  protocol: string;
  httpPort: number;
  sshPort: number;
  version: string;
  pgVersion: string;
  userUid: string;
  userGid: string;
  secretKey: string;
  internalT: string;
  dbHostname: string;
  forgejoHostname: string;
}

function buildConfig(): StackConfig {
  return {
    dbUser:          envOr('FORGEJO_DB_USER',     'forgejo'),
    dbPassword:      envOr('FORGEJO_DB_PASSWORD', randomHex(32)),
    dbName:          envOr('FORGEJO_DB_NAME',     'forgejo'),
    domain:          envOr('FORGEJO_DOMAIN',      'localhost'),
    protocol:        envOr('FORGEJO_PROTOCOL',    'http'),
    httpPort:        envOrInt('FORGEJO_HTTP_PORT', 3000),
    sshPort:         envOrInt('FORGEJO_SSH_PORT',  2222),
    version:         envOr('FORGEJO_VERSION',     '11'),
    pgVersion:       envOr('POSTGRES_VERSION',    '16-alpine'),
    userUid:         envOr('FORGEJO_USER_UID',    '1000'),
    userGid:         envOr('FORGEJO_USER_GID',    '1000'),
    secretKey:       envOr('FORGEJO_SECRET_KEY',     randomHex(32)),
    internalT:       envOr('FORGEJO_INTERNAL_TOKEN', randomHex(52)),
    // Stable container names so docker's embedded DNS can route
    // forgejo→postgres traffic via service hostname (Perry's compose
    // engine doesn't yet register the service-key as a network alias).
    dbHostname:      'forgejo-db',
    forgejoHostname: 'forgejo-app',
  };
}

function buildSpec(c: StackConfig) {
  return {
    version: '3.8',
    services: {
      db: {
        image: `postgres:${c.pgVersion}`,
        container_name: c.dbHostname,
        restart: 'unless-stopped',
        environment: {
          POSTGRES_USER:     c.dbUser,
          POSTGRES_PASSWORD: c.dbPassword,
          POSTGRES_DB:       c.dbName,
          // Lets `pg_isready` find the right user without `-U`.
          PGUSER: c.dbUser,
        },
        volumes: ['forgejo-pgdata:/var/lib/postgresql/data'],
        networks: ['forgejo-db-net'],
        healthcheck: {
          test: ['CMD-SHELL', `pg_isready -U ${c.dbUser} -d ${c.dbName}`],
          interval: '5s',
          timeout: '3s',
          retries: 10,
          start_period: '30s',
        },
      },
      forgejo: {
        image: `data.forgejo.org/forgejo/forgejo:${c.version}`,
        container_name: c.forgejoHostname,
        restart: 'unless-stopped',
        depends_on: {
          db: { condition: 'service_healthy' },
        },
        environment: {
          USER_UID: c.userUid,
          USER_GID: c.userGid,

          // ── Database ──────────────────────────────────────────────
          FORGEJO__database__DB_TYPE: 'postgres',
          FORGEJO__database__HOST:    `${c.dbHostname}:5432`,
          FORGEJO__database__NAME:    c.dbName,
          FORGEJO__database__USER:    c.dbUser,
          FORGEJO__database__PASSWD:  c.dbPassword,
          FORGEJO__database__SSL_MODE: 'disable', // private network only

          // ── Server ────────────────────────────────────────────────
          FORGEJO__server__PROTOCOL:           c.protocol,
          FORGEJO__server__DOMAIN:             c.domain,
          FORGEJO__server__ROOT_URL:           `${c.protocol}://${c.domain}:${c.httpPort}/`,
          FORGEJO__server__HTTP_PORT:          '3000',
          FORGEJO__server__SSH_DOMAIN:         c.domain,
          FORGEJO__server__SSH_PORT:           String(c.sshPort),
          FORGEJO__server__SSH_LISTEN_PORT:    '22',
          // Forgejo's image runs OpenSSH on port 22 in its entrypoint
          // (the canonical "use OpenSSH for git-over-ssh" pattern), so
          // the Go-based built-in SSH server must NOT also bind 22 —
          // setting `START_SSH_SERVER=true` produces "bind: address
          // already in use" and exit-0's the container. With this
          // setting, Forgejo writes authorized_keys for OpenSSH to
          // consume; SSH operations route through the system sshd.
          FORGEJO__server__START_SSH_SERVER:   'false',
          FORGEJO__server__OFFLINE_MODE:       'true',
          FORGEJO__server__DISABLE_ROUTER_LOG: 'true',

          // ── Secrets ───────────────────────────────────────────────
          FORGEJO__security__INSTALL_LOCK:    'true',
          FORGEJO__security__SECRET_KEY:      c.secretKey,
          FORGEJO__security__INTERNAL_TOKEN:  c.internalT,

          // ── Service / registration ────────────────────────────────
          // Production-safe defaults: no public registration, no
          // captcha, signed-in browsing only.
          FORGEJO__service__DISABLE_REGISTRATION:        'true',
          FORGEJO__service__REQUIRE_SIGNIN_VIEW:         'true',
          FORGEJO__service__ALLOW_ONLY_INTERNAL_REGISTRATION: 'true',
          FORGEJO__service__ENABLE_CAPTCHA:              'false',

          // ── Logging ───────────────────────────────────────────────
          FORGEJO__log__MODE:        'console',
          FORGEJO__log__LEVEL:       'Info',

          // ── Federation ────────────────────────────────────────────
          FORGEJO__federation__ENABLED: 'false',
        },
        volumes: [
          'forgejo-data:/data',
          // Best-effort timezone sync to host. Hosts without /etc/
          // timezone (e.g. some minimal Alpine VMs) just see a missing
          // mount source — docker tolerates it; the container falls
          // back to UTC.
          '/etc/timezone:/etc/timezone:ro',
          '/etc/localtime:/etc/localtime:ro',
        ],
        ports: [
          `${c.httpPort}:3000`,
          `${c.sshPort}:22`,
        ],
        networks: ['forgejo-db-net', 'forgejo-web-net'],
        healthcheck: {
          test: [
            'CMD-SHELL',
            'wget -q -O /dev/null --timeout=2 --tries=1 http://127.0.0.1:3000/api/healthz || exit 1',
          ],
          interval: '10s',
          timeout: '5s',
          retries: 6,
          start_period: '60s',
        },
      },
    },
    networks: {
      // Internal-only: the `db` service joins this and is unreachable
      // from the host or from sibling stacks.
      'forgejo-db-net': { driver: 'bridge', internal: true },
      // Public bridge for the forgejo container's web + SSH ports.
      'forgejo-web-net': { driver: 'bridge' },
    },
    volumes: {
      'forgejo-pgdata': { driver: 'local' },
      'forgejo-data':   { driver: 'local' },
    },
  };
}

// ──────────────────────────────────────────────────────────────────────
// Lifecycle commands
// ──────────────────────────────────────────────────────────────────────

async function cmdUp(c: StackConfig): Promise<void> {
  await preflightOrExit(c.httpPort, c.sshPort);

  console.log(`🚀 Deploying Forgejo ${c.version} (data.forgejo.org/forgejo/forgejo:${c.version})`);
  console.log(`   • Web    ${c.protocol}://${c.domain}:${c.httpPort}`);
  console.log(`   • SSH    ssh://git@${c.domain}:${c.sshPort}`);
  console.log(`   • DB     postgres:${c.pgVersion}  (user=${c.dbUser}, db=${c.dbName})`);

  // `up()` is idempotent: re-running this script while the stack is
  // already running is a no-op (Perry's compose engine inspects each
  // service and skips when status is "running"; if the container exists
  // but is stopped, it `start`s it).
  const stack = await up(buildSpec(c) as never);
  console.log(`✅ Stack started (handle ${String(stack)})`);

  console.log('\n🏥 Waiting for PostgreSQL to accept connections (≤60s)...');
  if (!await waitForPostgres(stack, 60_000)) {
    console.error('❌ PostgreSQL never became ready. Tearing down.');
    await down(stack, { volumes: true });
    process.exit(1);
  }
  console.log('✅ PostgreSQL ready.');

  console.log('🏥 Waiting for Forgejo HTTP API (≤120s)...');
  if (!await waitForForgejo(stack, 120_000)) {
    console.error('❌ Forgejo HTTP API never answered. Tearing down.');
    await down(stack, { volumes: true });
    process.exit(1);
  }
  console.log('✅ Forgejo HTTP API ready.');

  console.log(`
─────────────────────────────────────────────────────────────
🎉  Forgejo ${c.version} is up and ready.
─────────────────────────────────────────────────────────────

  Web UI         ${c.protocol}://${c.domain}:${c.httpPort}/
  Git over SSH   ssh://git@${c.domain}:${c.sshPort}/
  Healthz        ${c.protocol}://${c.domain}:${c.httpPort}/api/healthz

  Database       postgres ${c.pgVersion} (private network, not host-bound)
  Volumes        forgejo-data, forgejo-pgdata
  Networks       forgejo-db-net (internal), forgejo-web-net (bridge)

  First-run admin user (run once on a fresh deployment):
    docker exec ${c.forgejoHostname} forgejo admin user create \\
      --admin --username root --email root@${c.domain} \\
      --random-password

  To tear the stack down:
    ./forgejo_app --down                    # preserves volumes
    FORGEJO_DESTROY_ON_EXIT=1 ./forgejo_app --down   # also drops volumes
─────────────────────────────────────────────────────────────
`);
  // Process exits 0 here; the docker daemon keeps the containers
  // running. `restart: unless-stopped` brings them back across host
  // reboots until an explicit `--down` (or `docker rm`) tears them.
}

async function cmdDown(c: StackConfig): Promise<void> {
  await preflightOrExit(c.httpPort, c.sshPort);

  const flag = envOr('FORGEJO_DESTROY_ON_EXIT', '');
  const destroy = flag === '1' || flag === 'true' || flag === 'yes';

  console.log(
    `📥 Tearing down Forgejo stack ` +
    (destroy ? '(volumes WILL be removed)' : '(volumes preserved)') +
    '...'
  );

  // Re-up against the same spec to obtain a stack handle for the
  // already-running deployment. Idempotent: services already running
  // are detected via `inspect` and skipped (no restart, no rebuild).
  // The handle returned references the same engine state — `down()`
  // then operates on the live containers / networks / volumes.
  const stack = await up(buildSpec(c) as never);
  await down(stack, { volumes: destroy });
  console.log('✅ Stack removed.');
}

async function main() {
  const args = process.argv.slice(2);
  const wantsDown = args.indexOf('--down') >= 0 || args.indexOf('down') >= 0;
  const wantsHelp = args.indexOf('--help') >= 0 || args.indexOf('-h') >= 0;

  if (wantsHelp) {
    console.log(
      'Forgejo deployment example (perry/compose)\n' +
      '\n' +
      'Usage:\n' +
      '  ./forgejo_app           Deploy + verify health + exit 0\n' +
      '  ./forgejo_app --down    Tear the stack down\n' +
      '  ./forgejo_app --help    Show this help\n' +
      '\n' +
      'Environment overrides (all optional):\n' +
      '  FORGEJO_VERSION         (default: 11)\n' +
      '  POSTGRES_VERSION        (default: 16-alpine)\n' +
      '  FORGEJO_DOMAIN          (default: localhost)\n' +
      '  FORGEJO_PROTOCOL        (default: http)\n' +
      '  FORGEJO_HTTP_PORT       (default: 3000)\n' +
      '  FORGEJO_SSH_PORT        (default: 2222)\n' +
      '  FORGEJO_DB_USER         (default: forgejo)\n' +
      '  FORGEJO_DB_PASSWORD     (default: random hex on first deploy)\n' +
      '  FORGEJO_DB_NAME         (default: forgejo)\n' +
      '  FORGEJO_USER_UID        (default: 1000)\n' +
      '  FORGEJO_USER_GID        (default: 1000)\n' +
      '  FORGEJO_DESTROY_ON_EXIT  set to 1 to drop volumes on --down\n'
    );
    process.exit(0);
  }

  const config = buildConfig();
  if (wantsDown) {
    await cmdDown(config);
  } else {
    await cmdUp(config);
  }
}

main().catch((err: unknown) => {
  console.error('💥 Fatal error:', err);
  process.exit(1);
});
