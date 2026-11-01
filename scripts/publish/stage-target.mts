/**
 * @file Materialize one `@perryts/perry-target-<triple>` package from an
 *   extracted `perry-cross-<triple>.tar.gz` bundle. Called by
 *   scripts/stage-npm.sh (which extracts the bundle and zstd-compresses the
 *   archives afterwards); the pure part is exported for publish.test.mts.
 *
 *   Every bundle file is checked against the bundle's own manifest.json
 *   before it is copied, and the package's target.json is that manifest
 *   restated for the package layout (`lib/<name>`, hashes of the
 *   uncompressed archives). perry accepts an installed target package only
 *   when target.json names its own version, source revision and triple.
 */

import { createHash } from 'node:crypto'
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'

import { rootPath, targetPackageName } from './constants.mts'

/** One archive as both the bundle manifest and target.json describe it. */
export interface TargetFile {
  path: string
  sha256: string
  size: number
}

/** The package's target.json (the WASI bundle manifest schema). */
export interface TargetJson {
  schema_version: 1
  perry_version: string
  source_revision: string
  target_triple: string
  files: TargetFile[]
}

export interface StageTargetOptions {
  /** The extracted bundle: manifest.json plus the archives it lists. */
  bundleDir: string
  /** npm/perry-target-<triple>; replaced whole. */
  packageDir: string
  triple: string
  version: string
  /**
   * The release's source revision. Required when the bundle's manifest has
   * none (build-cross bundles before schema 1); when both exist they must agree.
   */
  revision?: string
  /** Repo root holding npm/perry-target/package.json.tmpl and LICENSE. */
  repoRoot?: string
}

const REVISION = /^[0-9a-f]{40}$/
const SHA256 = /^[0-9a-f]{64}$/

function sha256(file: string): string {
  return createHash('sha256').update(readFileSync(file)).digest('hex')
}

/** Check the bundle against its manifest and write the package. */
export function stageTarget(options: StageTargetOptions): TargetJson {
  const { bundleDir, packageDir, triple, version } = options
  const repoRoot = options.repoRoot ?? rootPath
  const where = `perry-cross-${triple}`
  const manifestPath = path.join(bundleDir, 'manifest.json')
  if (!existsSync(manifestPath)) throw new Error(`${where}: bundle has no manifest.json`)
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as Record<string, unknown>

  if (manifest.target_triple !== triple) {
    throw new Error(`${where}: manifest target_triple is ${String(manifest.target_triple)}`)
  }
  if (manifest.perry_version !== version) {
    throw new Error(
      `${where}: manifest perry_version is ${String(manifest.perry_version)}; staging ${version}`,
    )
  }
  const bundleRevision = manifest.source_revision
  if (bundleRevision !== undefined && typeof bundleRevision !== 'string') {
    throw new Error(`${where}: manifest source_revision is not a string`)
  }
  if (bundleRevision && options.revision && bundleRevision !== options.revision) {
    throw new Error(
      `${where}: bundle was built from ${bundleRevision}; this release is ${options.revision}`,
    )
  }
  const revision = bundleRevision || options.revision || ''
  if (!REVISION.test(revision)) {
    throw new Error(
      `${where}: no full source revision (bundle manifest has none; set PERRY_SOURCE_REVISION)`,
    )
  }

  const files = manifest.files
  if (!Array.isArray(files) || files.length === 0) {
    throw new Error(`${where}: manifest lists no files`)
  }
  const listed = new Set<string>()
  const checked: TargetFile[] = []
  for (const entry of files as Array<Record<string, unknown>>) {
    const name = entry?.path
    if (typeof name !== 'string' || name === '' || name !== path.basename(name) || name.startsWith('.')) {
      throw new Error(`${where}: manifest file path ${String(name)} is not a plain file name`)
    }
    if (typeof entry.sha256 !== 'string' || !SHA256.test(entry.sha256) ||
        typeof entry.size !== 'number' || !Number.isSafeInteger(entry.size) || entry.size < 0) {
      throw new Error(`${where}: manifest entry for ${name} has no valid sha256/size`)
    }
    if (listed.has(name)) throw new Error(`${where}: manifest lists ${name} twice`)
    listed.add(name)
    const file = path.join(bundleDir, name)
    if (!existsSync(file)) throw new Error(`${where}: ${name} is listed but missing`)
    const size = statSync(file).size
    const digest = sha256(file)
    if (size !== entry.size || digest !== entry.sha256) {
      throw new Error(
        `${where}: ${name} is ${size} bytes sha256 ${digest}; the manifest says ${entry.size} bytes ${entry.sha256}`,
      )
    }
    checked.push({ path: name, sha256: digest, size })
  }
  for (const name of readdirSync(bundleDir)) {
    if (name !== 'manifest.json' && !listed.has(name)) {
      throw new Error(`${where}: ${name} is in the bundle but not in its manifest`)
    }
  }
  if (!listed.has('libperry_runtime.a') && !listed.has('perry_runtime.lib')) {
    throw new Error(`${where}: bundle has no runtime archive`)
  }

  rmSync(packageDir, { recursive: true, force: true })
  mkdirSync(path.join(packageDir, 'lib'), { recursive: true })
  for (const file of checked) {
    copyFileSync(path.join(bundleDir, file.path), path.join(packageDir, 'lib', file.path))
  }

  const targetJson: TargetJson = {
    schema_version: 1,
    perry_version: version,
    source_revision: revision,
    target_triple: triple,
    files: checked.map(file => ({ ...file, path: `lib/${file.path}` })),
  }
  writeFileSync(path.join(packageDir, 'target.json'), `${JSON.stringify(targetJson, null, 2)}\n`)

  const template = readFileSync(
    path.join(repoRoot, 'npm', 'perry-target', 'package.json.tmpl'),
    'utf8',
  )
  const rendered = template.replaceAll('__TRIPLE__', triple).replaceAll('__VERSION__', version)
  const packageJson = JSON.parse(rendered) as { name?: string }
  if (packageJson.name !== targetPackageName(triple)) {
    throw new Error(`${where}: rendered package name ${String(packageJson.name)}`)
  }
  writeFileSync(path.join(packageDir, 'package.json'), rendered)

  const license = path.join(repoRoot, 'LICENSE')
  if (existsSync(license)) copyFileSync(license, path.join(packageDir, 'LICENSE'))
  writeFileSync(
    path.join(packageDir, 'README.md'),
    `# ${targetPackageName(triple)}

Prebuilt Perry libraries (runtime, stdlib, UI) for cross-compiling to
\`${triple}\` with \`perry compile --target\`.

Install the version that matches your perry exactly, next to it:

\`\`\`bash
npm install -g ${targetPackageName(triple)}@${version}   # global perry
npm install --save-dev ${targetPackageName(triple)}@${version}   # project perry
\`\`\`

Without npm, \`perry target add <target>\` installs the same package from the
GitHub release into \`~/.perry/targets\`.
`,
  )
  return targetJson
}

// --- CLI entry --------------------------------------------------------------
// node scripts/publish/stage-target.mts <bundle-dir> <package-dir> <triple> <version>
// (the revision comes from PERRY_SOURCE_REVISION when set)
import { fileURLToPath } from 'node:url'

const isMainModule =
  process.argv[1] === fileURLToPath(new URL(import.meta.url))

if (isMainModule) {
  const [bundleDir, packageDir, triple, version] = process.argv.slice(2)
  if (!bundleDir || !packageDir || !triple || !version || process.argv.length !== 6) {
    console.error('usage: stage-target.mts <bundle-dir> <package-dir> <triple> <version>')
    process.exit(2)
  }
  try {
    const staged = stageTarget({
      bundleDir,
      packageDir,
      triple,
      version,
      revision: process.env.PERRY_SOURCE_REVISION || undefined,
    })
    console.log(`  ${staged.files.length} file(s), source ${staged.source_revision}`)
  } catch (error) {
    console.error(`  error: ${(error as Error).message}`)
    process.exit(1)
  }
}
