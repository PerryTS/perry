/** Dispatch and watch an exact-commit nightly build on GitHub Actions. */

import process from 'node:process'

import { rootPath } from './constants.mts'
import { logger, runCapture } from './shared.mts'

const WORKFLOW = 'release-packages.yml'
const REPO = 'PerryTS/perry'

async function main(): Promise<void> {
  const clean = await runCapture('git', ['status', '--porcelain', '--untracked-files=all'], rootPath)
  if (clean.code !== 0 || clean.stdout.trim()) {
    logger.fail('Nightly builds require a clean checkout. Commit or remove local changes first.')
    process.exitCode = 1
    return
  }

  const branch = await runCapture('git', ['branch', '--show-current'], rootPath)
  const head = await runCapture('git', ['rev-parse', 'HEAD'], rootPath)
  const ref = branch.stdout.trim()
  const sha = head.stdout.trim()
  if (branch.code !== 0 || !ref || head.code !== 0 || !sha) {
    logger.fail('Nightly builds require a named branch checkout.')
    process.exitCode = 1
    return
  }

  const remote = await runCapture('git', ['ls-remote', '--heads', 'origin', `refs/heads/${ref}`], rootPath)
  const remoteSha = remote.stdout.trim().split(/\s+/)[0] ?? ''
  if (remote.code !== 0 || remoteSha !== sha) {
    logger.fail(`Push this exact commit first: local ${ref}@${sha}, origin has ${remoteSha || '<missing>'}.`)
    process.exitCode = 1
    return
  }

  const dispatchedAt = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  const dispatched = await runCapture('gh', [
    'workflow', 'run', WORKFLOW, '-R', REPO, '--ref', ref,
    '-f', 'nightly=true', '-f', `candidate_sha=${sha}`,
  ], rootPath)
  if (dispatched.code !== 0) {
    logger.fail(`Could not dispatch ${WORKFLOW}; check GitHub CLI authentication and workflow access.`)
    process.exitCode = 1
    return
  }

  logger.info(`Dispatched nightly build for ${ref}@${sha}. Resolving its Actions run…`)
  let runId = ''
  for (let attempt = 0; attempt < 20 && !runId; attempt += 1) {
    await new Promise(resolve => setTimeout(resolve, 3000))
    const listed = await runCapture('gh', [
      'run', 'list', '--workflow', WORKFLOW, '-R', REPO, '--event', 'workflow_dispatch',
      '--limit', '20', '--json', 'databaseId,createdAt,headBranch,headSha',
    ], rootPath)
    try {
      const runs = JSON.parse(listed.stdout) as Array<{
        createdAt: string; databaseId: number; headBranch: string; headSha: string
      }>
      const match = runs.find(run =>
        run.createdAt >= dispatchedAt && run.headBranch === ref && run.headSha === sha,
      )
      if (match) runId = String(match.databaseId)
    } catch {
      // GitHub may not list a newly queued run immediately; keep polling.
    }
  }
  if (!runId) {
    logger.fail('The workflow was dispatched, but its Actions run id did not appear. Check GitHub Actions.')
    process.exitCode = 1
    return
  }

  logger.info(`Watching ${runId}; this builds the release package matrix but skips stable gates and npm.`)
  const watched = await runCapture('gh', ['run', 'watch', runId, '-R', REPO, '--exit-status'], rootPath)
  if (watched.code !== 0) {
    logger.fail(`Nightly build failed. Inspect https://github.com/${REPO}/actions/runs/${runId}`)
    process.exitCode = 1
    return
  }
  logger.info(`Nightly build completed: https://github.com/${REPO}/releases?q=nightly`)
}

await main()
