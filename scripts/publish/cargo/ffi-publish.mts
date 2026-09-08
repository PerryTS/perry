/**
 * @file perry-ffi → crates.io publisher, ported from scripts/publish_perry_ffi.sh
 *   into the publish-script tree. Maintainer-only: needs ~/.cargo/credentials.toml
 *   with a crates.io API token (`cargo login` once).
 *
 *   PREREQUISITES: publish perry-native-registration, then perry-runtime,
 *   before perry-ffi. Cargo requires both the registration core and the
 *   optional runtime-link dependency to resolve on crates.io.
 *   Dependency publication is manual; for versions not already published:
 *     cargo publish --dry-run -p perry-native-registration
 *     cargo publish -p perry-native-registration
 *     cargo publish -p perry-runtime
 *     ./scripts/publish_perry_ffi.sh  (perry-ffi dry run)
 *     npm run publish:ffi            (perry-ffi publication)
 *   perry-runtime's own workspace dependencies need prior publication as
 *   appropriate, outside this entrypoint.
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
    'Prerequisites: the Cargo.toml versions of perry-native-registration and ' +
      'perry-runtime must already resolve on crates.io. Publish the registration ' +
      'core first, then the runtime, before perry-ffi.',
  )
  // Cargo verifies the package before publishing; the shell entrypoint above
  // provides the separate perry-ffi dry run after dependency publication.
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
