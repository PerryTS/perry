/**
 * perry/compose — build an image from a local Containerfile + run it
 *
 * Demonstrates the `build:` field on a service spec — Perry calls the
 * backend's image-build CLI (e.g. `docker build -t <tag> -f Containerfile
 * .`) before starting the container. The resulting image is tagged
 * `<service-key>-image` by default and used for the run.
 *
 * Files in this directory:
 *   main.ts    — this script
 *   Containerfile — minimal alpine image that prints a marker on start
 *
 * Run from this directory so the build context (`.`) is correct:
 *   cd examples/container/build
 *   perry main.ts -o build_app
 *   PERRY_CONTAINER_BACKEND=docker ./build_app
 *
 * Note on the `setTimeout` calls: Perry's runtime currently doesn't
 * keep the event loop alive purely on a pending FFI Promise — see
 * examples/container/simple/main.ts for the why.
 */

import { up, down, logs } from 'perry/compose';
import { getBackend } from 'perry/container';

async function main() {
  console.log('backend:', getBackend());
  console.log('building + starting app...');

  const stack = await up({
    version: '3.8',
    services: {
      app: {
        build: {
          context: '.',
          dockerfile: 'Containerfile',
          args: { BUILD_ENV: 'production' },
        },
        container_name: 'perry-example-build-app',
        environment: { NODE_ENV: 'production' },
      },
    },
  });
  console.log('stack handle:', String(stack));

  // Wait for the container's startup CMD to run + emit its marker.
  await new Promise((r) => setTimeout(r, 1500));

  // Tail logs to confirm the build wired the BUILD_ENV arg through.
  const logsJson = await logs(stack, { tail: 5 });
  console.log('logs:');
  const parsed = JSON.parse(logsJson);
  console.log(parsed.stdout.trim());

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
