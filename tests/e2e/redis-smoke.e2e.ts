// Minimal compose-lifecycle smoke. The harness asserts:
//   1. Compile + link succeed
//   2. Process exits 0
//   3. stdout contains "[e2e] PASS"

import { up, down } from 'perry/compose';
import { getBackend } from 'perry/container';

async function main() {
  console.log('backend:', getBackend());

  console.log('starting stack...');
  const port = process.env['PERRY_E2E_PORT'] || '57399';
  const stack = await up({
    version: '3.8',
    services: {
      cache: {
        image: 'redis:7-alpine',
        container_name: 'perry-e2e-cache',
        ports: [`${port}:6379`],
        networks: ['e2e-net'],
      },
    },
    networks: {
      'e2e-net': { driver: 'bridge' },
    },
  });
  console.log('stack handle:', String(stack));

  // Give redis a moment to bind. We don't probe the host port (which
  // would race with docker's bind setup); the contract is just that
  // up() returns successfully and down() tears the stack down clean.
  await new Promise((r) => setTimeout(r, 500));

  console.log('tearing down...');
  await down(stack, { volumes: false });
  console.log('done');

  console.log('[e2e] PASS');
}

main().catch((err) => {
  console.error('[e2e] FAIL:', err);
  process.exit(1);
});
