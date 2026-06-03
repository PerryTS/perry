/**
 * perry/compose — minimal single-service smoke test
 *
 * Brings up an nginx container, lists it, then tears it down. The
 * shortest possible end-to-end exercise of the perry/compose API.
 *
 * Run:
 *   perry main.ts -o simple
 *   ./simple                                    # uses platform default
 *   PERRY_CONTAINER_BACKEND=docker ./simple     # pin a specific runtime
 *
 * Note on the `setTimeout` calls below: Perry's runtime currently
 * doesn't keep the event loop alive purely on a pending FFI Promise,
 * so a small `setTimeout` after each container op gives the tokio
 * task time to complete before the next `await`. Without it, `up()`
 * silently exits before the container is created. This is a Perry
 * runtime issue (tracked separately); every working compose example
 * uses the same workaround.
 */

import { up, down, ps } from 'perry/compose';
import { getBackend } from 'perry/container';

async function main() {
  console.log('backend:', getBackend());
  console.log('starting stack...');

  const stack = await up({
    version: '3.8',
    services: {
      web: {
        image: 'nginx:alpine',
        container_name: 'perry-example-simple-nginx',
        ports: ['18080:80'],
      },
    },
  });
  console.log('stack handle:', String(stack));

  // Keep the runtime alive long enough for `up`'s tokio task to
  // settle the Promise (see header note).
  await new Promise((r) => setTimeout(r, 500));

  // ps returns a JSON-encoded ContainerInfo[] — parse it.
  const statuses = JSON.parse(await ps(stack));
  console.log('container status:');
  for (const s of statuses) {
    console.log(`  ${s.name}\t${s.status}`);
  }

  await new Promise((r) => setTimeout(r, 200));
  console.log('tearing down...');
  await down(stack, { volumes: false });
  console.log('done');
  console.log('PASS');
}

main().catch((err) => {
  console.error('FAIL:', err);
  process.exit(1);
});
