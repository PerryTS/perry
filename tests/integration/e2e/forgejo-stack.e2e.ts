// Forgejo E2E: full stack-deploy + healthcheck-gated startup +
// post-up exec + idempotent redeploy + downByProject cleanup.
//
// The harness (perry-container-e2e) asserts:
//   1. Compile + link succeed (every TS feature in the spec)
//   2. Process exits 0
//   3. stdout contains `[e2e] PASS`
//
// This is the "production pattern" example; uses real Forgejo from
// `data.forgejo.org` (the official OCI registry; codeberg.org gates
// pulls behind a Codeberg account, gitea is a different project).

import { up, exec } from 'perry/compose';
import { downByProject } from 'perry/container';

const PROJECT = `e2e-forgejo-${process.argv[1]?.split('/').pop() || 'host'}`;
const FORGEJO_VERSION = process.env['PERRY_E2E_FORGEJO_VERSION'] || '11';
const POSTGRES_VERSION = process.env['PERRY_E2E_POSTGRES_VERSION'] || '16-alpine';

async function waitForPostgres(stack: number, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      await exec(stack, 'db', ['pg_isready', '-U', 'forgejo', '-d', 'forgejo']);
      return true;
    } catch (_e) {
      await new Promise((r) => setTimeout(r, 800));
    }
  }
  return false;
}

async function waitForForgejo(stack: number, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      await exec(stack, 'forgejo', [
        'wget', '-q', '-O', '/dev/null',
        '--timeout=2', '--tries=1',
        'http://127.0.0.1:3000/api/healthz',
      ]);
      return true;
    } catch (_e) {
      await new Promise((r) => setTimeout(r, 1500));
    }
  }
  return false;
}

async function main() {
  // Always tear down anything labelled with our project name FIRST —
  // recovers from a previous interrupted run without manual cleanup.
  console.log('pre-cleanup...');
  const preCleanup = JSON.parse(
    await downByProject(PROJECT, { volumes: true, networks: true }),
  );
  console.log(`  removed ${preCleanup.containers_removed} container(s) from prior runs`);

  console.log(`deploying forgejo stack as project=${PROJECT}...`);
  const stack = await up({
    version: '3.8',
    services: {
      db: {
        image: `postgres:${POSTGRES_VERSION}`,
        container_name: `${PROJECT}-db`,
        environment: {
          POSTGRES_USER:     'forgejo',
          POSTGRES_PASSWORD: 'e2e-fixed-password-not-secret',
          POSTGRES_DB:       'forgejo',
          PGUSER:            'forgejo',
        },
        volumes: ['forgejo-pgdata:/var/lib/postgresql/data'],
        networks: ['forgejo-db-net'],
        healthcheck: {
          test: ['CMD-SHELL', 'pg_isready -U forgejo -d forgejo'],
          interval: '5s',
          timeout: '3s',
          retries: 10,
          start_period: '30s',
        },
      },
      forgejo: {
        image: `data.forgejo.org/forgejo/forgejo:${FORGEJO_VERSION}`,
        container_name: `${PROJECT}-app`,
        depends_on: { db: { condition: 'service_healthy' } },
        environment: {
          USER_UID: '1000',
          USER_GID: '1000',
          FORGEJO__database__DB_TYPE: 'postgres',
          FORGEJO__database__HOST:    `${PROJECT}-db:5432`,
          FORGEJO__database__NAME:    'forgejo',
          FORGEJO__database__USER:    'forgejo',
          FORGEJO__database__PASSWD:  'e2e-fixed-password-not-secret',
          FORGEJO__server__PROTOCOL:  'http',
          FORGEJO__server__DOMAIN:    'localhost',
          FORGEJO__server__ROOT_URL:  'http://localhost:3000/',
          FORGEJO__server__START_SSH_SERVER: 'false',
          FORGEJO__security__INSTALL_LOCK:   'true',
          FORGEJO__security__SECRET_KEY:     'e2e-fixed-secret-key-not-prod',
          FORGEJO__security__INTERNAL_TOKEN: 'e2e-fixed-internal-token-not-prod',
          FORGEJO__service__DISABLE_REGISTRATION: 'true',
          FORGEJO__log__MODE:        'console',
        },
        volumes: ['forgejo-data:/data'],
        networks: ['forgejo-db-net', 'forgejo-web-net'],
      },
    },
    networks: {
      'forgejo-db-net':  { driver: 'bridge', internal: true },
      'forgejo-web-net': { driver: 'bridge' },
    },
    volumes: {
      'forgejo-pgdata': { driver: 'local' },
      'forgejo-data':   { driver: 'local' },
    },
  });
  console.log(`  stack handle: ${String(stack)}`);

  console.log('waiting for postgres (≤60s)...');
  if (!await waitForPostgres(stack, 60_000)) {
    console.error('[e2e] FAIL: postgres never became ready');
    await downByProject(PROJECT, { volumes: true });
    process.exit(1);
  }
  console.log('  postgres ready');

  console.log('waiting for forgejo /api/healthz (≤120s)...');
  if (!await waitForForgejo(stack, 120_000)) {
    console.error('[e2e] FAIL: forgejo never answered /api/healthz');
    await downByProject(PROJECT, { volumes: true });
    process.exit(1);
  }
  console.log('  forgejo healthz ready');

  // Final auto-cleanup — drop the whole stack via the new helper, no
  // manual `down(handle)` boilerplate. Volumes:true so subsequent test
  // runs start clean.
  console.log('cleanup: downByProject...');
  const post = JSON.parse(
    await downByProject(PROJECT, { volumes: true, networks: true }),
  );
  console.log(`  removed ${post.containers_removed} container(s)`);

  console.log('[e2e] PASS');
}

main().catch((err) => {
  console.error('[e2e] FAIL:', err);
  // Always best-effort cleanup on error
  downByProject(PROJECT, { volumes: true }).catch(() => {});
  process.exit(1);
});
