#!/usr/bin/env node
/**
 * Umbrella updater for repository dependencies and upstream action references.
 *
 * Usage: pnpm run update [--dry-run]
 *        pnpm run check
 *        pnpm run update --actions-only | --deps-only
 */
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

function parseArgs(argv: string[]): { dryRun: boolean; actionsOnly: boolean; depsOnly: boolean } {
  const allowed = new Set(['--dry-run', '--actions-only', '--deps-only'])
  const unknown = argv.filter((arg) => !allowed.has(arg))
  if (unknown.length) throw new Error('unknown argument(s): ' + unknown.join(', '))
  const actionsOnly = argv.includes('--actions-only')
  const depsOnly = argv.includes('--deps-only')
  if (actionsOnly && depsOnly) throw new Error('--actions-only and --deps-only cannot be combined')
  return { dryRun: argv.includes('--dry-run'), actionsOnly, depsOnly }
}

function run(script: string, args: string[]): number {
  console.log('[update] node ' + script + (args.length ? ' ' + args.join(' ') : ''))
  const result = spawnSync(process.execPath, [script, ...args], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
  })
  if (result.error) console.error('[update] ' + result.error.message)
  return result.status ?? 1
}

export function runUpdate(argv = process.argv.slice(2)): number {
  const options = parseArgs(argv)
  const statuses: number[] = []
  const actionArgs = options.dryRun ? ['--fix', '--dry-run'] : ['--fix']
  if (!options.depsOnly) statuses.push(run('scripts/update-actions.mts', actionArgs))
  if (!options.actionsOnly) {
    const depsArgs = options.dryRun ? ['--dry-run'] : []
    statuses.push(run('scripts/soak/update-deps.mts', depsArgs))
  }
  return statuses.reduce((status, next) => status || next, 0)
}

const isMain = process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
if (isMain) {
  try {
    process.exitCode = runUpdate()
  } catch (error) {
    console.error('[update] ' + (error instanceof Error ? error.message : String(error)))
    process.exitCode = 1
  }
}
