/**
 * @file Materialize one `@perryts/perry-target-<triple>` package from an
 *   extracted `perry-cross-<triple>.tar.gz` bundle, or (`--ext`) one
 *   `@perryts/perry-ext-<triple>` package from a host's extracted release
 *   archive. Called by scripts/stage-npm.sh (which extracts the input and
 *   zstd-compresses the archives afterwards); the pure part is exported for
 *   the tests.
 *
 *   Target: every bundle file is checked against the bundle's own
 *   manifest.json before it is copied, and the package's target.json is that
 *   manifest restated for the package layout (`lib/<name>`, hashes of the
 *   uncompressed archives). Ext: every `libperry_ext_*` archive of the
 *   release archive is copied and hashed into the same target.json schema.
 *   perry accepts an installed package of either kind only when target.json
 *   names its own version, source revision and triple.
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

import { extPackageName, rootPath, targetPackageName } from './constants.mts'

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

  return writePackage({
    kind: 'target',
    sourceDir: bundleDir,
    packageDir,
    triple,
    version,
    revision,
    files: checked,
    repoRoot,
  })
}

export interface StageExtOptions {
  /** The extracted release archive of the host (perry + lib*.a). */
  sourceDir: string
  /** npm/perry-ext-<triple>; replaced whole. */
  packageDir: string
  triple: string
  version: string
  /** The release's full source revision (PERRY_SOURCE_REVISION); required. */
  revision?: string
  repoRoot?: string
}

/** The native-extension archives of a release archive. */
const EXT_ARCHIVE = /^(lib)?perry_ext_[a-z0-9_]+\.(a|lib)$/

/** Write the ext package of a host from its release archive's ext archives. */
export function stageExt(options: StageExtOptions): TargetJson {
  const { sourceDir, packageDir, triple, version } = options
  const where = extPackageName(triple)
  const revision = options.revision ?? ''
  if (!REVISION.test(revision)) {
    throw new Error(`${where}: no full source revision (set PERRY_SOURCE_REVISION)`)
  }
  const names = readdirSync(sourceDir).filter(name => EXT_ARCHIVE.test(name)).sort()
  if (names.length === 0) {
    throw new Error(`${where}: ${sourceDir} has no libperry_ext_* archives`)
  }
  const files = names.map(name => {
    const file = path.join(sourceDir, name)
    return { path: name, sha256: sha256(file), size: statSync(file).size }
  })
  return writePackage({
    kind: 'ext',
    sourceDir,
    packageDir,
    triple,
    version,
    revision,
    files,
    repoRoot: options.repoRoot ?? rootPath,
  })
}

interface WritePackage {
  kind: 'target' | 'ext'
  sourceDir: string
  packageDir: string
  triple: string
  version: string
  revision: string
  /** Checked files of `sourceDir`, by plain name. */
  files: TargetFile[]
  repoRoot: string
}

/** The package both kinds share: lib/, target.json, package.json, README, LICENSE. */
function writePackage(p: WritePackage): TargetJson {
  const name = p.kind === 'target' ? targetPackageName(p.triple) : extPackageName(p.triple)
  rmSync(p.packageDir, { recursive: true, force: true })
  mkdirSync(path.join(p.packageDir, 'lib'), { recursive: true })
  for (const file of p.files) {
    copyFileSync(path.join(p.sourceDir, file.path), path.join(p.packageDir, 'lib', file.path))
  }

  const targetJson: TargetJson = {
    schema_version: 1,
    perry_version: p.version,
    source_revision: p.revision,
    target_triple: p.triple,
    files: p.files.map(file => ({ ...file, path: `lib/${file.path}` })),
  }
  writeFileSync(path.join(p.packageDir, 'target.json'), `${JSON.stringify(targetJson, null, 2)}\n`)

  const template = readFileSync(
    path.join(p.repoRoot, 'npm', `perry-${p.kind}`, 'package.json.tmpl'),
    'utf8',
  )
  const rendered = template.replaceAll('__TRIPLE__', p.triple).replaceAll('__VERSION__', p.version)
  const packageJson = JSON.parse(rendered) as { name?: string }
  if (packageJson.name !== name) {
    throw new Error(`${name}: rendered package name ${String(packageJson.name)}`)
  }
  writeFileSync(path.join(p.packageDir, 'package.json'), rendered)

  const license = path.join(p.repoRoot, 'LICENSE')
  if (existsSync(license)) copyFileSync(license, path.join(p.packageDir, 'LICENSE'))
  const readme =
    p.kind === 'target'
      ? `# ${name}

Prebuilt Perry libraries (runtime, stdlib, UI) for cross-compiling to
\`${p.triple}\` with \`perry compile --target\`.

Install the version that matches your perry exactly, next to it:

\`\`\`bash
npm install -g ${name}@${p.version}   # global perry
npm install --save-dev ${name}@${p.version}   # project perry
\`\`\`

Without npm, \`perry target add <target>\` installs the same package from the
GitHub release into \`~/.perry/targets\`.
`
      : `# ${name}

Prebuilt native-extension archives (\`libperry_ext_*.a\`) for perry on
\`${p.triple}\`: the host functions behind node:http, ws, net, zlib, sharp,
pdf, the database drivers and the other well-known modules.

You normally do not install this package yourself: the first compile that
needs one of these archives downloads this exact package from perry's GitHub
release, verifies its signature and installs it into \`~/.perry/ext\`
(\`PERRY_NO_AUTO_FETCH=1\` turns that off). To install it ahead of time, next
to perry, use the version that matches your perry exactly:

\`\`\`bash
npm install -g ${name}@${p.version}   # global perry
npm install --save-dev ${name}@${p.version}   # project perry
perry target add --ext   # without npm
\`\`\`
`
  writeFileSync(path.join(p.packageDir, 'README.md'), readme)
  return targetJson
}

// --- CLI entry --------------------------------------------------------------
// node scripts/publish/stage-target.mts [--ext] <source-dir> <package-dir> <triple> <version>
// (the revision comes from PERRY_SOURCE_REVISION when set)
import { fileURLToPath } from 'node:url'

const isMainModule =
  process.argv[1] === fileURLToPath(new URL(import.meta.url))

if (isMainModule) {
  const args = process.argv.slice(2)
  const ext = args[0] === '--ext'
  if (ext) args.shift()
  const [sourceDir, packageDir, triple, version] = args
  if (!sourceDir || !packageDir || !triple || !version || args.length !== 4) {
    console.error('usage: stage-target.mts [--ext] <source-dir> <package-dir> <triple> <version>')
    process.exit(2)
  }
  const revision = process.env.PERRY_SOURCE_REVISION || undefined
  try {
    const staged = ext
      ? stageExt({ sourceDir, packageDir, triple, version, revision })
      : stageTarget({ bundleDir: sourceDir, packageDir, triple, version, revision })
    console.log(`  ${staged.files.length} file(s), source ${staged.source_revision}`)
  } catch (error) {
    console.error(`  error: ${(error as Error).message}`)
    process.exit(1)
  }
}
