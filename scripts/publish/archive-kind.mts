/** Archive ownership, shared with the compiler through npm/archive-kinds.rules. */
import { copyFileSync, mkdirSync, readFileSync, readdirSync, realpathSync, rmSync, statSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export type ArchiveKind = 'host' | 'target' | 'ui' | 'wasm-host' | 'ext'
const rules = readFileSync(new URL('../../npm/archive-kinds.rules', import.meta.url), 'utf8')
  .split('\n').map(line => line.trim()).filter(line => line && !line.startsWith('#'))
  .map(line => line.split(/\s+/) as [ArchiveKind, string])

export function archiveKind(name: string): ArchiveKind | undefined {
  const raw = name.replace(/\.gc-effects$/, '').replace(/\.zst$/, '')
  if (!/\.(a|lib)$/.test(raw)) return undefined
  const stem = raw.replace(/\.(a|lib)$/, '').replace(/^lib/, '')
  for (const [kind, pattern] of rules) {
    if (pattern.endsWith('*') ? stem.startsWith(pattern.slice(0, -1)) : stem === pattern) return kind
  }
}

/** Release archives contain host files only; build artifacts retain all kinds for npm staging. */
export function stageHost(source: string, dest: string): void {
  if (path.resolve(source) === path.resolve(dest)) throw new Error("host staging requires a separate destination")
  rmSync(dest, { recursive: true, force: true })
  mkdirSync(dest, { recursive: true })
  for (const name of readdirSync(source)) {
    if (!statSync(path.join(source, name)).isFile()) continue
    if (archiveKind(name) === 'host' || /^(perry(\.exe)?|LLVM-C\.dll)$/.test(name)) {
      copyFileSync(path.join(source, name), path.join(dest, name))
    }
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  const [command, source, dest] = process.argv.slice(2)
  if (command === '--stage-host' && source && dest) stageHost(source, dest)
  else if (command === '--host-libs' && source) {
    for (const name of readdirSync(source)) if (archiveKind(name) === 'host') console.log(name)
  } else { console.error('usage: archive-kind.mts --stage-host <source> <dest> | --host-libs <source>'); process.exit(2) }
}
