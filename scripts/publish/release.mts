/**
 * @file Post-publish release orchestration for Perry, modeled on a tiered registry-infra design
 *   Derives the GitHub release body from changelog.d
 *   fragments (via scripts/cut_release_notes.sh --notes-only — Perry froze
 *   CHANGELOG.md at v0.5.1264), then creates the git tag + the IMMUTABLE
 *   (draft → upload → undraft) GitHub release carrying packaging/install.sh +
 *   a checksums file.
 *
 *   The platform tarballs (perry-macos-aarch64.tar.gz, …) are built in CI, not
 *   locally — a follow-up CI leg uploads them to this release. The local cut
 *   uploads the assets the author HAS locally: install.sh + checksums.txt.
 *
 *   The tag + release are the LAST markers of a release, so they may only
 *   exist once the version is resolvable on npm (requireRegistryLive). A
 *   STAGED package is not published — staging may never be approved.
 */

import { createHash } from 'node:crypto'
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { INSTALL_SH, RELEASE_REPO, rootPath } from './constants.mts'
import { logger, runCapture } from './shared.mts'
import { fetchPublishedVersion } from './npm/shared.mts'

/** Concatenate changelog.d/ fragments into the release body. */
export async function extractReleaseNotes(cwd: string = rootPath): Promise<string> {
  const { stdout, code } = await runCapture(
    'sh',
    ['scripts/cut_release_notes.sh', '--notes-only'],
    cwd,
  )
  if (code !== 0 || !stdout.trim()) {
    logger.warn('cut_release_notes.sh --notes-only failed; falling back to a one-liner.')
    return ''
  }
  return stdout
}

/**
 * Registry-liveness gate: poll npm view until <name>@<version> is resolvable,
 * or fail loud after attempts. npm propagation lags a few seconds behind an
 * approve. Returns true when every (name, version) pair is live.
 */
export async function requireRegistryLive(
  packages: ReadonlyArray<{ name: string; version: string }>,
  config: { attempts?: number; delayMs?: number } = {},
): Promise<boolean> {
  const { attempts = 12, delayMs = 5000 } = config
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    const live = await Promise.all(
      packages.map(async p => [p, await fetchPublishedVersion(p.name, p.version)] as const),
    )
    const notLive = live.filter(([, ok]) => !ok).map(([{ name, version }]) => `${name}@${version}`)
    if (notLive.length === 0) {
      logger.info(`registry liveness: all ${packages.length} package(s) resolvable (attempt ${attempt}).`)
      return true
    }
    if (attempt === attempts) {
      logger.fail(
        `registry liveness: ${notLive.join(', ')} never turned up after ${attempts} attempts — do NOT cut the release.`,
      )
      return false
    }
    logger.info(`registry liveness: waiting on ${notLive.join(', ')} (attempt ${attempt}/${attempts})…`)
    await new Promise(r => setTimeout(r, delayMs))
  }
  return false
}

/** Compute sha256 for a file (hex). */
function sha256File(file: string): string {
  return createHash('sha256').update(readFileSync(file)).digest('hex')
}

/** Write a checksums.txt (sha256) for the given asset paths, return its path. */
export function writeChecksums(files: readonly string[], cwd: string = rootPath): string {
  const out = path.join(cwd, 'checksums.txt')
  const lines = files.map(f => `${sha256File(f)}  ${path.basename(f)}`)
  writeFileSync(out, lines.join('\n') + '\n')
  return out
}

/**
 * Create the tag + immutable GitHub release (draft → upload → undraft).
 * Assets: packaging/install.sh + checksums.txt. The platform tarballs are
 * uploaded by a follow-up CI leg. Returns true only when the tag is on origin
 * AND the release is published (or already existed).
 */
export async function ensureTagAndRelease(version: string): Promise<boolean> {
  const tagName = `v${version}`

  // 1. Assets FIRST. install.sh is a REQUIRED release asset (the release
  //    advertises `curl ... install.sh | sh`), and the tag is immutable once
  //    pushed — so it must not be cut when a required asset is absent. Fail
  //    here, before any tag exists, rather than warning after the tag is live.
  const installSh = path.join(rootPath, INSTALL_SH)
  if (!existsSync(installSh)) {
    logger.fail(
      `install.sh not found at ${installSh} — refusing to cut the release.\n` +
        `  Why: install.sh is a required release asset (the release advertises \`curl ... install.sh | sh\`).\n` +
        `  A tag is immutable, so it must not be pushed without the required assets.\n` +
        `  Fix: restore packaging/install.sh and re-run.`,
    )
    process.exitCode = 1
    return false
  }
  const checksums = writeChecksums([installSh])
  const assets: string[] = [installSh, checksums]

  try {
    // 2. Tag (on HEAD — the bump commit is already on main).
    const tagCheck = await runCapture(
      'git',
      ['rev-parse', '-q', '--verify', `refs/tags/${tagName}`],
      rootPath,
    )
    if (tagCheck.code !== 0) {
      const created = await runCapture('git', ['tag', tagName], rootPath)
      if (created.code !== 0) {
        logger.fail(`could not create tag ${tagName}`)
        process.exitCode = 1
        return false
      }
      logger.log(`Created tag ${tagName}.`)
    }

    // 3. Push the tag. Tolerate a non-zero push only when origin already
    //    carries it AT THE SAME COMMIT as the local tag — a remote tag at a
    //    different commit means someone else tagged a different release, and
    //    continuing would publish the release against the wrong commit.
    const pushed = await runCapture('git', ['push', 'origin', tagName], rootPath)
    if (pushed.code !== 0) {
      const remote = await runCapture(
        'git',
        ['ls-remote', '--tags', 'origin', `refs/tags/${tagName}`],
        rootPath,
      )
      if (remote.code !== 0 || !remote.stdout.includes(`refs/tags/${tagName}`)) {
        logger.fail(
          `could not push tag ${tagName} to origin (git push exited ${pushed.code}) and origin does not carry it.\n` +
            `  Fix: resolve the push (auth? protected ref? network?) and re-run.`,
        )
        process.exitCode = 1
        return false
      }
      // Peel both sides to a commit SHA and compare. For a lightweight tag the
      // ls-remote line IS the commit; for an annotated tag the `^{}` line is.
      const localCommit = (
        await runCapture('git', ['rev-parse', `refs/tags/${tagName}^{commit}`], rootPath)
      ).stdout.trim()
      let remoteCommit = ''
      for (const line of remote.stdout.split('\n')) {
        const peeled = line.match(/^([0-9a-f]+)\trefs\/tags\/[^\t]+\^\{\}/)
        if (peeled) {
          remoteCommit = peeled[1]!
          break
        }
      }
      if (!remoteCommit) {
        const raw = remote.stdout.split('\n')[0]
        remoteCommit = raw ? (raw.split(/\s/)[0] ?? '') : ''
      }
      if (remoteCommit !== localCommit) {
        logger.fail(
          `tag ${tagName} on origin points to ${remoteCommit || '<unknown>'}, not the local commit ${localCommit} — ` +
            `refusing to publish the release against the wrong commit.\n` +
            `  Fix: land a version bump (this tag name is taken) or reconcile the remote tag.`,
        )
        process.exitCode = 1
        return false
      }
      logger.log(`Tag ${tagName} already on origin at the expected commit; continuing.`)
    }

    // 4. If the release already exists, resume a draft or leave a published one.
    //    A tag-only check treats an interrupted draft as complete and skips the
    //    asset upload + undraft — so inspect isDraft and finish the draft here.
    const view = await runCapture(
      'gh',
      ['release', 'view', tagName, '--json', 'tagName,isDraft'],
      rootPath,
    )
    if (view.code === 0) {
      let isDraft = false
      try {
        isDraft = (JSON.parse(view.stdout) as { isDraft?: boolean }).isDraft === true
      } catch {
        /* unreadable → treat as published (do not re-publish a live release) */
      }
      if (!isDraft) {
        logger.log(`Release ${tagName} already published; leaving it untouched.`)
        return true
      }
      logger.log(`Release ${tagName} exists as a draft — resuming asset upload + publish.`)
      // --clobber: the interrupted run may have already uploaded install.sh /
      // checksums.txt; overwrite them (checksums.txt is regenerated each run).
      const upload = await runCapture('gh', ['release', 'upload', tagName, '--clobber', ...assets], rootPath)
      if (upload.code !== 0) {
        logger.fail(`gh release upload failed (${upload.code})`)
        process.exitCode = 1
        return false
      }
      const undraft = await runCapture('gh', ['release', 'edit', tagName, '--draft=false'], rootPath)
      if (undraft.code !== 0) {
        logger.fail(`gh release edit --draft=false failed (${undraft.code})`)
        process.exitCode = 1
        return false
      }
      logger.log(
        `Release ${tagName} published (resumed from draft). ` +
          `curl -fsSL https://github.com/${RELEASE_REPO}/releases/download/${tagName}/install.sh | sh`,
      )
      return true
    }

    // 5. Release notes from changelog.d fragments.
    const notes = await extractReleaseNotes()
    const notesFile = path.join(os.tmpdir(), `release-notes-${version}.md`)
    writeFileSync(notesFile, notes || `Release ${version}.`)

    // 6. Immutable release: draft → upload → undraft.
    const create = await runCapture(
      'gh',
      ['release', 'create', tagName, '--draft', '--verify-tag', '--title', tagName, '--notes-file', notesFile],
      rootPath,
    )
    if (create.code !== 0) {
      logger.fail(`gh release create failed (${create.code})`)
      process.exitCode = 1
      return false
    }
    const upload = await runCapture('gh', ['release', 'upload', tagName, '--clobber', ...assets], rootPath)
    if (upload.code !== 0) {
      logger.fail(`gh release upload failed (${upload.code})`)
      process.exitCode = 1
      return false
    }
    const undraft = await runCapture('gh', ['release', 'edit', tagName, '--draft=false'], rootPath)
    if (undraft.code !== 0) {
      logger.fail(`gh release edit --draft=false failed (${undraft.code})`)
      process.exitCode = 1
      return false
    }
    logger.log(
      `Release ${tagName} published from changelog.d fragments. ` +
        `curl -fsSL https://github.com/${RELEASE_REPO}/releases/download/${tagName}/install.sh | sh`,
    )
    return true
  } finally {
    // checksums.txt is written into the repo tree solely for the upload.
    for (const a of assets) {
      if (path.basename(a) === 'checksums.txt') {
        try {
          rmSync(a, { force: true })
        } catch {
          /* best-effort */
        }
      }
    }
  }
}
