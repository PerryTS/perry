// Runtime setup shared by the standalone require regressions. Cargo unifies
// features per invocation: building a missing wrapper separately can create a
// second Tokio runtime even when every archive has the same source stamp.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

export function prepareRequireRuntime(root) {
  // Direct script users may supply their own verified, coherent archives.
  // The CI-visible Rust entry points explicitly request the build below.
  if (process.env.PERRY_TEST_BUILD_RUNTIME !== '1') return;
  // perry-dev keeps panic=abort and avoids a second thin-LTO build inside
  // cargo test. Release/dist remain available for optimization-sensitive runs.
  const profile = process.env.PERRY_TEST_RUNTIME_PROFILE ?? 'perry-dev';
  if (!['release', 'perry-dev', 'dist'].includes(profile)) {
    throw new Error('Native require tests need a panic=abort runtime profile');
  }
  const packages = [
    'perry-runtime', 'perry-stdlib', 'perry-runtime-static', 'perry-stdlib-static',
    'perry-ext-events', 'perry-ext-http', 'perry-ext-net',
    'perry-ext-typescript', 'perry-ext-ws', 'perry-ext-zlib',
  ];
  const args = ['build', '--locked', '--profile', profile,
    ...packages.flatMap(name => ['-p', name])];
  if (process.env.PERRY_TEST_WASM === '1') {
    args.push('-p', 'perry-wasm-host', '--features', 'perry-runtime/wasm-host');
  }
  const buildEnv = { ...process.env };
  // CI overrides target rustflags to disable lld. Such an override replaces
  // .cargo/config.toml's unwind/frame-pointer flags rather than extending it.
  // Preserve those native exception/stack-walking requirements in every
  // override while keeping the caller's linker and other options intact.
  for (const key of Object.keys(buildEnv)) {
    if (key === 'RUSTFLAGS' || /^CARGO_TARGET_.+_RUSTFLAGS$/.test(key)) {
      buildEnv[key] += ' -C force-unwind-tables=yes -C force-frame-pointers=yes';
    }
  }
  if (buildEnv.CARGO_ENCODED_RUSTFLAGS !== undefined) {
    buildEnv.CARGO_ENCODED_RUSTFLAGS += '\x1f-C\x1fforce-unwind-tables=yes' +
      '\x1f-C\x1fforce-frame-pointers=yes';
  }
  console.log('Building coherent require-test runtime archives...');
  const build = spawnSync(process.env.CARGO ?? 'cargo', args, {
    cwd: root, env: buildEnv, encoding: 'utf8',
    timeout: 600_000, killSignal: 'SIGKILL', maxBuffer: 8 * 1024 * 1024,
  });
  if (build.error || build.status !== 0) {
    throw new Error('Coherent runtime build failed: ' + (build.error ?? build.status) +
      '\n' + (build.stdout ?? '') + (build.stderr ?? ''));
  }
  const target = path.resolve(root, process.env.CARGO_TARGET_DIR ?? 'target');
  const runtime = path.join(target, profile);
  // Refuse a misleading successful Cargo invocation (e.g. a cross-target
  // override) rather than falling back to an unrelated installed archive.
  for (const name of ['runtime', 'stdlib', 'ext_events', 'ext_http', 'ext_net',
    'ext_typescript', 'ext_ws', 'ext_zlib']) {
    const filename = process.platform === 'win32' ? 'perry_' + name + '.lib' :
      'libperry_' + name + '.a';
    fs.accessSync(path.join(runtime, filename), fs.constants.R_OK);
  }
  process.env.PERRY_RUNTIME_DIR = runtime;
  console.log('Using coherent require-test runtime: ' + runtime);
}
