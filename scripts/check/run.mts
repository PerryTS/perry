#!/usr/bin/env node
/**
 * Umbrella runner for standalone repository checks in scripts/check/.
 *
 * Usage: pnpm run check
 */
import { spawnSync } from 'node:child_process'
import { readdir } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
export async function discoverChecks(): Promise<string[]> {
  const directory = path.dirname(fileURLToPath(import.meta.url))
  return (await readdir(directory, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith('.mts') && entry.name !== 'run.mts')
    .map((entry) => path.join('scripts/check', entry.name))
    .sort()
}

export async function runChecks(): Promise<number> {
  let status = 0
  for (const script of await discoverChecks()) {
    console.log('[check] node ' + script)
    const result = spawnSync(process.execPath, [script], {
      cwd: REPO_ROOT,
      stdio: 'inherit',
    })
    if (result.error) console.error('[check] ' + result.error.message)
    const currentStatus = result.status ?? 1
    if (currentStatus !== 0) status = currentStatus
  }
  return status
}

const isMain = process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
if (isMain) runChecks().then((code) => { process.exitCode = code })
