/**
 * perry/compose — multi-service stack with named volumes + env interpolation
 *
 * Two services on a user-defined network:
 *   - db  : postgres with a named volume for durable state
 *   - web : nginx pointing at the postgres host (cross-service DNS)
 *
 * Demonstrates:
 *   - `${VAR:-default}` env interpolation (resolved at the FFI boundary
 *     against process.env before the spec hits the engine)
 *   - Named volume that survives `down(stack, { volumes: false })` and
 *     is removed by `down(stack, { volumes: true })`
 *   - User-defined network so cross-service DNS works (web → db:5432)
 *   - `depends_on` for explicit startup ordering
 *
 * Run:
 *   perry main.ts -o multi-service
 *   ./multi-service                                 # uses platform default
 *   DB_PASSWORD=hunter2 ./multi-service             # override interpolation
 *   PERRY_CONTAINER_BACKEND=docker ./multi-service  # pin runtime
 *
 * Note: the small `setTimeout` calls between FFI awaits keep the
 * runtime event loop alive while tokio tasks settle the Promises —
 * see examples/container/simple/main.ts for the why.
 */

import { up, down, logs } from 'perry/compose';
import { getBackend } from 'perry/container';

async function main() {
  console.log('backend:', getBackend());
  console.log('starting db + web stack...');

  const stack = await up({
    version: '3.8',
    services: {
      db: {
        image: 'postgres:16-alpine',
        container_name: 'perry-example-multi-db',
        environment: {
          POSTGRES_USER: '${DB_USER:-myuser}',
          POSTGRES_PASSWORD: '${DB_PASSWORD:-secret}',
          POSTGRES_DB: 'mydb',
        },
        volumes: ['db-data:/var/lib/postgresql/data'],
        ports: ['15432:5432'],
        networks: ['app-net'],
      },
      web: {
        // Public-image stand-in for "your app." Real apps swap this
        // for their own image; the rest of the spec stays the same.
        image: 'nginx:alpine',
        container_name: 'perry-example-multi-web',
        depends_on: ['db'],
        ports: ['13000:80'],
        environment: {
          DATABASE_URL: 'postgres://${DB_USER:-myuser}:${DB_PASSWORD:-secret}@db:5432/mydb',
        },
        networks: ['app-net'],
      },
    },
    networks: {
      'app-net': { driver: 'bridge' },
    },
    volumes: {
      // Empty `{}` here would trip a Perry runtime auto-stringification
      // bug; use any non-empty config instead. The default driver on
      // every backend is "local" — declaring it explicitly makes the
      // spec robust.
      'db-data': { driver: 'local' },
    },
  });
  console.log('stack handle:', String(stack));

  // Keep loop alive while up's tokio task settles.
  await new Promise((r) => setTimeout(r, 1000));

  // Drain logs from both services.
  const logsJson = await logs(stack, { tail: 5 });
  console.log('logs (last 5 lines):');
  const parsed = JSON.parse(logsJson);
  console.log('  stdout (head):', parsed.stdout.slice(0, 200));

  await new Promise((r) => setTimeout(r, 200));
  console.log('tearing down (and dropping the db-data volume)...');
  await down(stack, { volumes: true });
  console.log('done');
  console.log('PASS');
}

main().catch((err) => {
  console.error('FAIL:', err);
  process.exit(1);
});
