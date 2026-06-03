/**
 * perry/compose — production Forgejo (self-hosted Git) stack
 *
 * Two-service stack:
 *   - postgres : durable database for Forgejo state
 *   - forgejo  : web UI + git server (uses postgres for everything)
 *
 * Image source:
 *   data.forgejo.org/forgejo/forgejo:<MAJOR>
 *   The official Forgejo OCI registry. Don't use codeberg.org's mirror
 *   (gated behind a Codeberg account; intermittent 401s for public
 *   pulls) and don't use any Gitea-branded image (Forgejo forked from
 *   Gitea but their images now diverge in security patches).
 *
 * Run:
 *   perry main.ts -o forgejo_app
 *   PERRY_CONTAINER_BACKEND=docker ./forgejo_app
 *
 * After ~30s the stack is up. Visit http://localhost:13000 to see
 * the Forgejo install page (auto-completed via INSTALL_LOCK=true).
 *
 * Cleanup (the script exits 0 leaving the stack running, by design —
 * deploy + verify + exit 0 pattern):
 *   docker rm -f perry-fjo-pg perry-fjo-web
 *   docker volume ls -q | grep -E "(pgdata|fjodata)" | xargs -I{} docker volume rm {}
 *   docker network ls -q --filter name=fjonet | xargs -I{} docker network rm {}
 *
 * Production note: the random secrets generated below MUST be
 * stabilised across redeploys for any non-dev use. Forgejo's data dir
 * stores config encrypted with FORGEJO_SECRET_KEY; postgres rows are
 * authored under POSTGRES_PASSWORD. Re-running with different values
 * against the same volumes will corrupt state. Set via .env or a
 * secrets manager:
 *   openssl rand -hex 32   # FORGEJO_DB_PASSWORD, FORGEJO_SECRET_KEY
 *   openssl rand -hex 52   # FORGEJO_INTERNAL_TOKEN
 */

import { up } from 'perry/compose';
import { getBackend } from 'perry/container';

async function main() {
  console.log('backend:', getBackend());
  console.log('starting Forgejo stack (~30s on cold pull)...');

  const FORGEJO_VERSION = process.env['FORGEJO_VERSION'] || '11';
  const POSTGRES_VERSION = process.env['POSTGRES_VERSION'] || '16-alpine';

  const stack = await up({
    version: '3.8',
    services: {
      db: {
        image: `postgres:${POSTGRES_VERSION}`,
        container_name: 'perry-fjo-pg',
        restart: 'unless-stopped',
        environment: {
          POSTGRES_USER: '${FORGEJO_DB_USER:-forgejo}',
          POSTGRES_PASSWORD: '${FORGEJO_DB_PASSWORD:-changeme}',
          POSTGRES_DB: '${FORGEJO_DB_NAME:-forgejo}',
        },
        volumes: ['pgdata:/var/lib/postgresql/data'],
        networks: ['fjonet'],
      },
      forgejo: {
        image: `data.forgejo.org/forgejo/forgejo:${FORGEJO_VERSION}`,
        container_name: 'perry-fjo-web',
        depends_on: ['db'],
        restart: 'unless-stopped',
        environment: {
          // Cross-service DNS — postgres reachable by container_name
          // on the user-defined fjonet bridge.
          FORGEJO__database__DB_TYPE: 'postgres',
          FORGEJO__database__HOST: 'perry-fjo-pg:5432',
          FORGEJO__database__NAME: '${FORGEJO_DB_NAME:-forgejo}',
          FORGEJO__database__USER: '${FORGEJO_DB_USER:-forgejo}',
          FORGEJO__database__PASSWD: '${FORGEJO_DB_PASSWORD:-changeme}',
          FORGEJO__server__PROTOCOL: 'http',
          FORGEJO__server__DOMAIN: 'localhost',
          FORGEJO__server__ROOT_URL: 'http://localhost:13000/',
          FORGEJO__server__HTTP_PORT: '3000',
          // Disable Forgejo's built-in SSH server — the image's
          // entrypoint runs openssh on :22 which conflicts otherwise
          // (container exit 0 with "bind: address already in use").
          FORGEJO__server__START_SSH_SERVER: 'false',
          FORGEJO__security__INSTALL_LOCK: 'true',
          FORGEJO__service__DISABLE_REGISTRATION: 'true',
        },
        volumes: ['fjodata:/data'],
        ports: ['13000:3000'],
        networks: ['fjonet'],
      },
    },
    networks: {
      fjonet: { driver: 'bridge' },
    },
    volumes: {
      pgdata: { driver: 'local' },
      fjodata: { driver: 'local' },
    },
  });
  console.log('stack handle:', String(stack));

  // Keep the loop alive while up's tokio task settles + give the
  // services time to come online before exit.
  await new Promise((r) => setTimeout(r, 5000));

  console.log('');
  console.log('═════════════════════════════════════════════════════');
  console.log('Forgejo stack is up.');
  console.log('  Web UI : http://localhost:13000');
  console.log('  DB     : perry-fjo-pg:5432 (internal only)');
  console.log('═════════════════════════════════════════════════════');
  console.log('PASS');
}

main().catch((err) => {
  console.error('FAIL:', err);
  process.exit(1);
});
