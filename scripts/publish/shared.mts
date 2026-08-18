/**
 * @file Registry-agnostic publish helpers: interactive + capturing process
 *   spawns, git introspection, first-JSON extraction from noisy CLI output,
 *   and the logger/root-path shared by every publish module. Ported from the
 *   a tiered registry-infra design, but uses Node built-ins (no
 *   @socketsecurity/lib-stable spawn/logger coupling) so the generic core
 *   stays lean. The Socket SDK is pulled in only by scan.mts.
 */

import { spawn } from 'node:child_process'
import { fstatSync, readFileSync } from 'node:fs'
import process from 'node:process'

import { NPM_MIN_VERSION, rootPath } from './constants.mts'

export { rootPath }

/** Minimal logger — the publish scripts run interactively; console is enough. */
export const logger = {
  log: (...args: unknown[]): void => console.log(...args),
  warn: (...args: unknown[]): void => console.warn(...args),
  fail: (...args: unknown[]): void => console.error(...args),
  info: (...args: unknown[]): void => console.log(...args),
}

const WIN32 = process.platform === 'win32'

/** The staged→approve handoff block, printed once when a staging run finishes. */
export function formatApproveHandoff(
  approveCommand: string,
  ownership: string,
  repoPath: string = rootPath,
): string[] {
  return [`Next: cd ${repoPath} && ${approveCommand}`, ownership]
}

export function logApproveHandoff(
  approveCommand: string,
  ownership: string,
  repoPath: string = rootPath,
): void {
  for (const line of formatApproveHandoff(approveCommand, ownership, repoPath)) {
    logger.log(line)
  }
}

/** Spawn a command, forwarding stdio (interactive). Returns the exit code. */
export function runInherit(
  cmd: string,
  args: string[],
  cwd: string,
  env?: NodeJS.ProcessEnv | undefined,
): Promise<number> {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd,
      ...(env ? { env: { ...process.env, ...env } } : {}),
      shell: WIN32,
      stdio: 'inherit',
    })
    child.on('error', reject)
    child.on('exit', code => resolve(code ?? 0))
  })
}

export interface TeedRun {
  code: number
  output: string
}

/** Spawn a command, forward its output live, AND keep a copy. */
export function runInheritTee(
  cmd: string,
  args: string[],
  cwd: string,
  env?: NodeJS.ProcessEnv | undefined,
): Promise<TeedRun> {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd,
      ...(env ? { env: { ...process.env, ...env } } : {}),
      shell: WIN32,
      stdio: ['inherit', 'pipe', 'pipe'],
    })
    let output = ''
    child.stdout?.on('data', (chunk: Buffer) => {
      output += chunk.toString('utf8')
      process.stdout.write(chunk)
    })
    child.stderr?.on('data', (chunk: Buffer) => {
      output += chunk.toString('utf8')
      process.stderr.write(chunk)
    })
    child.on('error', reject)
    child.on('exit', code => resolve({ code: code ?? 0, output }))
  })
}

/**
 * Wrap a command in `script(1)`'s pseudo-terminal so npm's registry web-OTP
 * challenge stays alive off a non-interactive agent shell. Passthrough when
 * stdio is already a TTY, and on Windows (no script(1) — Windows runs
 * interactive-only).
 */
export function buildPtyInvocation(
  platform: NodeJS.Platform,
  cmd: string,
  args: readonly string[],
): { args: string[]; command: string } | undefined {
  if (platform === 'win32') return undefined
  if (platform === 'darwin') {
    // BSD script: `script -q /dev/null <cmd> <args…>` runs cmd directly.
    return { args: ['-q', '/dev/null', cmd, ...args], command: 'script' }
  }
  // util-linux script: command goes through `-c` as a single shell string.
  const quoted = [cmd, ...args]
    .map(a => `'${a.replace(/'/g, `'\\''`)}'`)
    .join(' ')
  return { args: ['-qec', quoted, '/dev/null'], command: 'script' }
}

export const NON_INTERACTIVE_RENDER_ENV: NodeJS.ProcessEnv = {
  NO_COLOR: '1',
}

/** True when fd 1 is a regular FILE (a `> out.log` redirect / agent capture). */
export function stdoutIsFileBacked(): boolean {
  try {
    return fstatSync(1).isFile()
  } catch {
    return false
  }
}

export const PTY_FILE_STDOUT_MESSAGE =
  'stdout is a file — pumping the PTY through a pipe.\n' +
  '  What:  script(1) cannot allocate a pseudo-terminal onto a file-backed\n' +
  '         stdout, so the wrapper gives the PTY child a PIPE and pumps its\n' +
  '         output into the file itself. The browser web-OTP flow proceeds.'

export function runPtyPumped(
  pty: { command: string; args: readonly string[] },
  cwd: string,
  env?: NodeJS.ProcessEnv | undefined,
): Promise<number> {
  return new Promise((resolve, reject) => {
    const child = spawn(pty.command, [...pty.args], {
      cwd,
      ...(env ? { env: { ...process.env, ...env } } : {}),
      stdio: ['inherit', 'pipe', 'pipe'],
    })
    child.stdout?.on('data', (chunk: Buffer) => process.stdout.write(chunk))
    child.stderr?.on('data', (chunk: Buffer) => process.stderr.write(chunk))
    child.on('error', reject)
    child.on('exit', code => resolve(code ?? 0))
  })
}

/** Spawn interactively, guaranteeing the child sees a TTY when one is absent. */
export function runInheritTty(
  cmd: string,
  args: string[],
  cwd: string,
  env?: NodeJS.ProcessEnv | undefined,
): Promise<number> {
  if (process.stdin.isTTY || WIN32) {
    return runInherit(cmd, args, cwd, env)
  }
  const pty = buildPtyInvocation(process.platform, cmd, args)
  if (!pty) return runInherit(cmd, args, cwd, env)
  if (stdoutIsFileBacked()) {
    logger.log(`[pty] ${PTY_FILE_STDOUT_MESSAGE}`)
    return runPtyPumped(pty, cwd, env)
  }
  return runInherit(pty.command, pty.args, cwd, env)
}

/** Spawn a command and capture stdout; stderr stays visible. */
export function runCapture(
  cmd: string,
  args: string[],
  cwd: string,
): Promise<{ stdout: string; code: number }> {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd,
      shell: WIN32,
      stdio: ['ignore', 'pipe', 'inherit'],
    })
    let stdout = ''
    child.stdout?.on('data', (chunk: Buffer) => {
      stdout += chunk.toString('utf8')
    })
    child.on('error', reject)
    child.on('exit', code => resolve({ stdout, code: code ?? 0 }))
  })
}

/** Compare two `major.minor.patch` versions. Returns a < 0, 0, or > 0. */
export function compareSemver(a: string, b: string): number {
  const pa = a.split('.').map(n => parseInt(n, 10) || 0)
  const pb = b.split('.').map(n => parseInt(n, 10) || 0)
  for (let i = 0; i < 3; i += 1) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0)
    if (d !== 0) return d
  }
  return 0
}

/**
 * Enforce the npm floor for the publish flow (NPM_MIN_VERSION). Returns a
 * non-empty string reason when npm is missing or too old, or undefined when
 * the floor is satisfied.
 */
export async function checkNpmFloor(): Promise<string | undefined> {
  const { stdout, code } = await runCapture('npm', ['--version'], rootPath)
  if (code !== 0 || !stdout.trim()) {
    return 'could not run `npm --version` — is npm on PATH?'
  }
  const version = stdout.trim().replace(/^v/, '')
  if (compareSemver(version, NPM_MIN_VERSION) < 0) {
    return (
      `npm ${version} is below the publish-flow floor of ${NPM_MIN_VERSION}.\n` +
      `  Why: the flow needs npm staged publishing (\`npm stage\`, >= 11.15.0),\n` +
      `  OIDC trusted publishing (>= 11.5.1), and \`min-release-age\` in DAYS\n` +
      `  (>= 11.17) — ${NPM_MIN_VERSION} covers all three.\n` +
      `  Fix: npm install -g npm@latest (or npm@${NPM_MIN_VERSION}).`
    )
  }
  return undefined
}

/** Resolve `git rev-parse --short HEAD`, or `unknown` when git fails. */
export async function gitShortSha(cwd: string): Promise<string> {
  const { stdout, code } = await runCapture(
    'git',
    ['rev-parse', '--short', 'HEAD'],
    cwd,
  )
  return code === 0 ? stdout.trim() : 'unknown'
}

/**
 * Extract the first balanced top-level JSON value (`{ … }` or `[ … ]`) from a
 * noisy stdout stream (npm stage list wraps JSON in progress lines). `npm stage
 * list --json` emits an ARRAY of staged entries, so `[` must be honored as a
 * start token — otherwise only the first object inside the array is extracted
 * and every staged entry after it disappears. Returns undefined if none found.
 */
export function extractFirstJson(text: string): string | undefined {
  const objIdx = text.indexOf('{')
  const arrIdx = text.indexOf('[')
  let startIdx: number
  if (objIdx === -1 && arrIdx === -1) return undefined
  else if (objIdx === -1) startIdx = arrIdx
  else if (arrIdx === -1) startIdx = objIdx
  else startIdx = Math.min(objIdx, arrIdx)
  const open = text[startIdx]!
  const close = open === '{' ? '}' : ']'
  let depth = 0
  let inString = false
  let escape = false
  for (let i = startIdx, { length } = text; i < length; i += 1) {
    const ch = text[i]!
    if (escape) {
      escape = false
      continue
    }
    if (ch === '\\') {
      escape = true
      continue
    }
    if (ch === '"') {
      inString = !inString
      continue
    }
    if (inString) continue
    if (ch === open) depth += 1
    else if (ch === close) {
      depth -= 1
      if (depth === 0) return text.slice(startIdx, i + 1)
    }
  }
  return undefined
}

/**
 * Whether this CI run may request npm provenance. The sigstore bundle is
 * verifiable only when the source repository is PUBLIC. Fail-closed outside
 * Actions or when the event payload is unreadable.
 */
export function provenanceAllowed(): boolean {
  if (process.env['GITHUB_ACTIONS'] !== 'true') return false
  const eventPath = process.env['GITHUB_EVENT_PATH']
  if (!eventPath) return false
  try {
    const event = JSON.parse(readFileSync(eventPath, 'utf8')) as {
      repository?:
        | { private?: boolean | undefined; visibility?: string | undefined }
        | undefined
    }
    const repo = event.repository
    if (!repo) return false
    if (repo.visibility !== undefined) return repo.visibility === 'public'
    return repo.private === false
  } catch {
    return false
  }
}
