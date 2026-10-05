#!/usr/bin/env node
/**
 * Refresh pinned upstream action reference snapshots.
 * Workflows never execute from upstream/; Perry-owned implementations live in
 * .github/actions/perry/ and simpler steps remain inline.
 */
import { spawnSync } from 'node:child_process'
import { cp, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

type ActionPin = {
  path: string
  implementationPath: string
  version: string
  tag: string
  sha: string
  source: string
  [key: string]: unknown
}
type PinFile = { schema: number; policy: string; actions: Record<string, ActionPin> }

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..')
const PINS_PATH = path.join(REPO_ROOT, 'upstream/actions/pins.json')
const SPARSE_PATHS = [
  '/action.yml', '/action.yaml', '/**/action.yml', '/**/action.yaml',
  '/dist/**', '/**/dist/**', '/restore/action.yml', '/save/action.yml',
  '/restore/dist/**', '/save/dist/**', '/src/**', '/scripts/**',
  '/externals/**', '/*.js', '/*.ps1', '/*.sh', '/LICENSE*', '/NOTICE*',
  '/node_modules/**', '/.gitmodules',
]

async function workflowFiles(directory: string): Promise<string[]> {
  const files: string[] = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const fullPath = path.join(directory, entry.name)
    if (entry.isDirectory()) files.push(...await workflowFiles(fullPath))
    else if (/\.(?:yml|yaml)$/.test(entry.name)) files.push(fullPath)
  }
  return files
}

async function externalActionReferences(): Promise<string[]> {
  const refs: string[] = []
  for (const file of await workflowFiles(path.join(REPO_ROOT, '.github'))) {
    const content = await readFile(file, 'utf8')
    content.split(/\r?\n/).forEach((line, index) => {
      const match = line.match(/^\s*(?:-\s*)?uses:\s*([^\s#]+)/)
      if (!match) return
      const ref = match[1]!
      if (!ref.startsWith('./') && !ref.startsWith('docker://')) {
        refs.push(path.relative(REPO_ROOT, file) + ':' + (index + 1) + ' ' + ref)
      }
      if (ref.includes('upstream/')) {
        refs.push(path.relative(REPO_ROOT, file) + ':' + (index + 1) + ' runtime reference into upstream/')
      }
    })
  }
  return refs
}

function runGit(args: string[], cwd?: string, input?: string): string {
  const result = spawnSync('git', args, {
    cwd, encoding: 'utf8', input, maxBuffer: 16 * 1024 * 1024,
  })
  if (result.error) throw result.error
  if (result.status !== 0) {
    throw new Error('git ' + args.join(' ') + ' failed: ' + result.stderr.trim())
  }
  return result.stdout
}

export function commitForTagListing(listing: string, tag: string): string | undefined {
  const baseRef = 'refs/tags/' + tag
  let base: string | undefined
  let peeled: string | undefined
  for (const line of listing.split(/\r?\n/)) {
    const match = line.match(/^([0-9a-f]{40})\s+(.+)$/)
    if (!match) continue
    if (match[2] === baseRef + '^{}') peeled = match[1]
    else if (match[2] === baseRef) base = match[1]
  }
  return peeled ?? base
}

function resolveTag(repository: string, tag: string): string {
  const listing = runGit([
    'ls-remote', 'https://github.com/' + repository + '.git',
    'refs/tags/' + tag, 'refs/tags/' + tag + '^{}',
  ])
  const sha = commitForTagListing(listing, tag)
  if (!sha) throw new Error('could not resolve ' + repository + '@' + tag)
  return sha
}

async function refreshSnapshot(pin: ActionPin, sha: string): Promise<void> {
  const tempRoot = await mkdtemp(path.join(os.tmpdir(), 'perry-action-pin-'))
  const source = path.join(tempRoot, 'source')
  const destination = path.join(REPO_ROOT, pin.path)
  try {
    runGit(['init', source])
    runGit(['remote', 'add', 'origin', pin.source], source)
    runGit(['fetch', '--depth=1', '--filter=blob:none', 'origin', sha], source)
    runGit(['checkout', '--detach', 'FETCH_HEAD'], source)
    runGit(['sparse-checkout', 'init', '--no-cone'], source)
    runGit(['sparse-checkout', 'set', '--no-cone', '--stdin'], source, SPARSE_PATHS.join('\n') + '\n')
    await rm(destination, { recursive: true, force: true })
    await cp(source, destination, {
      recursive: true,
      filter: (entry) => !entry.split(path.sep).includes('.git'),
    })
    await writeFile(path.join(destination, '.upstream-sha'), sha + '\n')
  } finally {
    await rm(tempRoot, { recursive: true, force: true })
  }
}

function parseArgs(argv: string[]): { check: boolean; dryRun: boolean } {
  const known = new Set(['--check', '--fix', '--dry-run'])
  const unknown = argv.filter((arg) => !known.has(arg))
  if (unknown.length) throw new Error('unknown argument(s): ' + unknown.join(', '))
  if (argv.includes('--check') && argv.includes('--fix')) {
    throw new Error('choose either --check or --fix')
  }
  return { check: argv.includes('--check'), dryRun: argv.includes('--dry-run') }
}

async function inspectActionPins(check: boolean): Promise<{
  pins: PinFile
  resolved: Map<string, string>
  errors: string[]
}> {
  const pins = JSON.parse(await readFile(PINS_PATH, 'utf8')) as PinFile
  const externalRefs = await externalActionReferences()
  if (externalRefs.length) {
    return {
      pins,
      resolved: new Map(),
      errors: externalRefs.map((ref) => 'external action reference: ' + ref),
    }
  }
  const resolved = new Map<string, string>()
  const errors: string[] = []
  for (const [name, pin] of Object.entries(pins.actions)) {
    try {
      const sha = resolveTag(name, pin.tag)
      resolved.set(name, sha)
      if (sha !== pin.sha) {
        const message = name + ' ' + pin.tag + ': ' + pin.sha + ' -> ' + sha
        if (check) errors.push(message)
        else console.log('[actions:update] ' + message)
      } else {
        console.log('[actions:update] ' + name + ' ' + pin.tag + ' unchanged (' + sha + ')')
      }
      const snapshotMarker = path.join(REPO_ROOT, pin.path, '.upstream-sha')
      let snapshotSha = ''
      try { snapshotSha = (await readFile(snapshotMarker, 'utf8')).trim() } catch {
        if (check) errors.push(name + ': missing reference snapshot marker at ' + pin.path)
      }
      if (snapshotSha && snapshotSha !== sha) {
        const message = name + ': reference snapshot is ' + snapshotSha + ', selected tag resolves to ' + sha
        if (check) errors.push(message)
        else console.log('[actions:update] ' + message)
      }
      if (!String(pin.usage ?? '').startsWith('Inline')) {
        const implementationMarker = path.join(REPO_ROOT, pin.implementationPath, '.upstream-sha')
        let implementationSha = ''
        try { implementationSha = (await readFile(implementationMarker, 'utf8')).trim() } catch {
          errors.push(name + ': missing Perry implementation source marker at ' + pin.implementationPath)
          continue
        }
        if (implementationSha !== sha) {
          const message = name + ': Perry implementation is based on ' + implementationSha +
            '; upstream reference is ' + sha + ' (review/adapt implementation)'
          if (check) errors.push(message)
          else console.warn('[actions:update] ' + message)
        }
      }
    } catch (error) {
      errors.push(name + ': ' + (error instanceof Error ? error.message : String(error)))
    }
  }
  return { pins, resolved, errors }
}

export async function checkActionPins(): Promise<number> {
  const { errors } = await inspectActionPins(true)
  if (errors.length) {
    for (const error of errors) console.error('[actions:update] ' + error)
    return 1
  }
  return 0
}

export async function updateActionPins(argv = process.argv.slice(2)): Promise<number> {
  const { check, dryRun } = parseArgs(argv)
  if (check) return checkActionPins()
  const { pins, resolved, errors } = await inspectActionPins(false)
  if (errors.length) {
    for (const error of errors) console.error('[actions:update] ' + error)
    return 1
  }
  if (dryRun) {
    console.log('[actions:update] dry run; no pins or snapshots changed')
    return 0
  }
  for (const [name, pin] of Object.entries(pins.actions)) {
    const sha = resolved.get(name)!
    const markerPath = path.join(REPO_ROOT, pin.path, '.upstream-sha')
    let snapshotSha = ''
    try { snapshotSha = (await readFile(markerPath, 'utf8')).trim() } catch {
      // Older snapshots need one canonical refresh.
    }
    if (sha !== pin.sha || snapshotSha !== sha) {
      await refreshSnapshot(pin, sha)
      pin.sha = sha
      console.log('[actions:update] refreshed ' + name + ' reference snapshot')
    }
  }
  await writeFile(PINS_PATH, JSON.stringify(pins, null, 2) + '\n')
  return 0
}

const isMain = process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
if (isMain) {
  updateActionPins().then((status) => {
    process.exitCode = status
  }).catch((error: unknown) => {
    console.error('[actions:update] ' + (error instanceof Error ? error.message : String(error)))
    process.exitCode = 1
  })
}
