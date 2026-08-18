/**
 * @file perry-ffi → crates.io publisher, ported from scripts/publish_perry_ffi.sh
 *   into the publish-script tree. Maintainer-only: needs ~/.cargo/credentials.toml
 *   with a crates.io API token (`cargo login` once).
 *
 *   PREREQUISITE: publish perry-runtime first. perry-ffi has an optional dep on
 *   perry-runtime gated by the `runtime-link` feature; cargo publish rejects
 *   perry-ffi until the matching perry-runtime version exists on crates.io.
 *   perry-runtime's own workspace-crate deps need similar handling (out of
 *   scope here — the order is documented, not automated).
 *
 *   Usage: npm run publish:ffi
 */

import process from 'node:process'

import { rootPath } from '../constants.mts'
import { logger, runInherit } from '../shared.mts'
import { readCargoVersion } from '../npm/bump.mts'

async function main(): Promise<void> {
  const version = readCargoVersion(rootPath)
  if (!version) {
    logger.fail('could not parse workspace version from Cargo.toml.')
    process.exitCode = 1
    return
  }
  logger.log(`Workspace version: ${version}`)
  logger.warn(
    'Prerequisite: perry-runtime@' + version + ' must already exist on crates.io ' +
      '(perry-ffi optional-deps gate). Publish it first if it does not.',
  )
  // Verify the package builds + would publish cleanly, then publish.
  // --allow-dirty: this script runs from a clean main right after a release
  // commit, but the worktree may still have generated CHANGELOG/Cargo.lock
  // changes from the auto-optimize pass.
  const code = await runInherit('cargo', ['publish', '-p', 'perry-ffi', '--allow-dirty'], rootPath)
  if (code !== 0) {
    logger.fail(`cargo publish -p perry-ffi failed (exit ${code}).`)
    process.exitCode = 1
    return
  }
  logger.log(`perry-ffi@${version} published to crates.io.`)
}

// Only run when invoked directly (node scripts/publish/cargo/ffi-publish.mts),
// not when imported for testing.
import { fileURLToPath } from 'node:url'
if (process.argv[1] === fileURLToPath(new URL(import.meta.url))) {
  await main()
}
