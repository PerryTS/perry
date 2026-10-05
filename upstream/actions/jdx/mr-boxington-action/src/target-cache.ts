import {chmod, copyFile, open, readdir, rm, stat, truncate, utimes} from 'node:fs/promises'
import path from 'node:path'

const BUILD_SCRIPT_REAL_SUFFIX = '.mbx-real'

interface CargoMetadata {
  packages: {
    name: string
    targets: {name: string}[]
  }[]
}

interface KeepSets {
  packages: Set<string>
  dependencies: Set<string>
  registry: Set<string>
}

function crateName(name: string): string {
  return name.replaceAll('-', '_')
}

export function cargoKeepSets(rawMetadata: string): KeepSets {
  const metadata = JSON.parse(rawMetadata) as CargoMetadata
  const packages = new Set<string>()
  const dependencies = new Set<string>()
  const registry = new Set<string>()
  for (const pkg of metadata.packages) {
    registry.add(pkg.name)
    for (const name of [pkg.name, ...pkg.targets.map(target => target.name)]) {
      packages.add(name)
      packages.add(crateName(name))
      dependencies.add(crateName(name))
      dependencies.add(`lib${crateName(name)}`)
    }
  }
  return {packages, dependencies, registry}
}

export function matchesCargoEntry(name: string, keep: Set<string>): boolean {
  if (keep.has(name)) return true
  const separator = name.lastIndexOf('-')
  return separator > 0 && keep.has(name.slice(0, separator))
}

async function remove(entry: string): Promise<void> {
  await rm(entry, {recursive: true, force: true})
}

async function directoryExists(directory: string): Promise<boolean> {
  try {
    return (await stat(directory)).isDirectory()
  } catch {
    return false
  }
}

async function pruneNamedDirectory(directory: string, keep: Set<string>): Promise<void> {
  if (!(await directoryExists(directory))) return
  for (const entry of await readdir(directory, {withFileTypes: true})) {
    if (!matchesCargoEntry(entry.name, keep)) await remove(path.join(directory, entry.name))
  }
}

async function pruneProfile(directory: string, keep: KeepSets): Promise<void> {
  for (const entry of await readdir(directory, {withFileTypes: true})) {
    if (!['build', '.fingerprint', 'deps', '.mbx-build-script-shims'].includes(entry.name)) {
      await remove(path.join(directory, entry.name))
    }
  }
  await pruneNamedDirectory(path.join(directory, 'build'), keep.packages)
  await pruneNamedDirectory(path.join(directory, '.fingerprint'), keep.packages)
  await pruneNamedDirectory(path.join(directory, 'deps'), keep.dependencies)
}

async function pruneTargetDirectory(directory: string, keep: KeepSets): Promise<void> {
  for (const entry of await readdir(directory, {withFileTypes: true})) {
    const child = path.join(directory, entry.name)
    if (!entry.isDirectory()) {
      if (entry.name !== 'CACHEDIR.TAG') await remove(child)
      continue
    }
    const isProfile = await Promise.all(
      ['build', '.fingerprint', 'deps'].map(marker => directoryExists(path.join(child, marker)))
    )
    if (isProfile.some(Boolean)) await pruneProfile(child, keep)
    else await pruneTargetDirectory(child, keep)
  }
}

async function pruneSparseIndex(
  directory: string,
  keep: Set<string>,
  isRoot = false
): Promise<boolean> {
  let empty = true
  for (const entry of await readdir(directory, {withFileTypes: true})) {
    const child = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      if (await pruneSparseIndex(child, keep)) await remove(child)
      else empty = false
    } else if (
      keep.has(entry.name) ||
      (isRoot && ['config.json', 'CACHEDIR.TAG'].includes(entry.name))
    ) {
      empty = false
    } else {
      await remove(child)
    }
  }
  return empty
}

export async function hasReusableCargoTarget(targetDirectory: string): Promise<boolean> {
  if (!(await directoryExists(targetDirectory))) return false
  for (const entry of await readdir(targetDirectory, {withFileTypes: true})) {
    if (!entry.isDirectory()) continue
    const child = path.join(targetDirectory, entry.name)
    if (entry.name === '.fingerprint') {
      if ((await readdir(child)).length > 0) return true
      continue
    }
    if (!['build', 'deps', '.mbx-build-script-shims'].includes(entry.name)) {
      if (await hasReusableCargoTarget(child)) return true
    }
  }
  return false
}

async function visitShimDirectories(
  directory: string,
  visitor: (shimDirectory: string) => Promise<void>
): Promise<void> {
  if (!(await directoryExists(directory))) return
  for (const entry of await readdir(directory, {withFileTypes: true})) {
    if (!entry.isDirectory()) continue
    const child = path.join(directory, entry.name)
    if (entry.name === '.mbx-build-script-shims') {
      for (const identity of await readdir(child, {withFileTypes: true})) {
        if (identity.isDirectory()) await visitor(path.join(child, identity.name))
      }
    } else if (!['build', '.fingerprint', 'deps'].includes(entry.name)) {
      await visitShimDirectories(child, visitor)
    }
  }
}

async function visitBuildScriptShims(
  directory: string,
  visitor: (shim: string) => Promise<void>
): Promise<void> {
  if (!(await directoryExists(directory))) return
  for (const entry of await readdir(directory, {withFileTypes: true})) {
    const child = path.join(directory, entry.name)
    if (entry.isFile() && entry.name.endsWith(BUILD_SCRIPT_REAL_SUFFIX)) {
      await visitor(child.slice(0, -BUILD_SCRIPT_REAL_SUFFIX.length))
    } else if (
      entry.isDirectory() &&
      !['.fingerprint', 'deps', '.mbx-build-script-shims'].includes(entry.name)
    ) {
      await visitBuildScriptShims(child, visitor)
    }
  }
}

async function isShellLauncher(file: string): Promise<boolean> {
  try {
    const handle = await open(file, 'r')
    try {
      const prefix = Buffer.alloc(2)
      const {bytesRead} = await handle.read(prefix, 0, prefix.length, 0)
      return bytesRead === prefix.length && prefix.equals(Buffer.from('#!'))
    } finally {
      await handle.close()
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false
    throw error
  }
}

function cargoBuildScriptAlias(shim: string): string {
  return path.join(path.dirname(shim), process.platform === 'win32' ? 'build-script-build.exe' : 'build-script-build')
}

async function installCachedBinary(source: string, destination: string): Promise<void> {
  await copyFile(source, destination)
  if (process.platform !== 'win32') await chmod(destination, 0o755)
}

export async function dehydrateMbxShimBinaries(targetDirectory: string): Promise<number> {
  let removed = 0
  await visitShimDirectories(targetDirectory, async shimDirectory => {
    const binary = path.join(shimDirectory, process.platform === 'win32' ? 'mbx.exe' : 'mbx')
    try {
      await rm(binary)
      removed += 1
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
    }
  })
  // mbx releases before the shared Unix launcher put a complete mbx binary at
  // every Cargo build-script path. The preserved `.mbx-real` sibling identifies
  // those paths without guessing names. Keep newer tiny shell launchers intact.
  await visitBuildScriptShims(targetDirectory, async shim => {
    if (await isShellLauncher(shim)) return
    for (const installed of [shim, cargoBuildScriptAlias(shim)]) {
      try {
        const metadata = await stat(installed)
        if (metadata.size === 0) continue
        await truncate(installed, 0)
        await utimes(installed, metadata.atime, metadata.mtime)
        removed += 1
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      }
    }
  })
  return removed
}

export async function hydrateMbxShimBinaries(
  targetDirectory: string,
  sourceBinary: string
): Promise<number> {
  let hydrated = 0
  await visitShimDirectories(targetDirectory, async shimDirectory => {
    const binary = path.join(shimDirectory, process.platform === 'win32' ? 'mbx.exe' : 'mbx')
    await copyFile(sourceBinary, binary)
    if (process.platform !== 'win32') await chmod(binary, 0o755)
    hydrated += 1
  })
  await visitBuildScriptShims(targetDirectory, async shim => {
    for (const installed of [shim, cargoBuildScriptAlias(shim)]) {
      let metadata
      try {
        metadata = await stat(installed)
        if (metadata.size > 0) continue
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      }
      await installCachedBinary(sourceBinary, installed)
      if (metadata) await utimes(installed, metadata.atime, metadata.mtime)
      hydrated += 1
    }
  })
  return hydrated
}

export async function pruneCargoTargetCache(
  targetDirectory: string,
  cargoHome: string,
  rawMetadata: string
): Promise<void> {
  const keep = cargoKeepSets(rawMetadata)
  if (await directoryExists(targetDirectory)) await pruneTargetDirectory(targetDirectory, keep)

  const indexes = path.join(cargoHome, 'registry', 'index')
  if (!(await directoryExists(indexes))) return
  for (const entry of await readdir(indexes, {withFileTypes: true})) {
    if (!entry.isDirectory()) continue
    const index = path.join(indexes, entry.name)
    if (await directoryExists(path.join(index, '.git'))) {
      await remove(path.join(index, '.cache'))
    } else {
      await pruneSparseIndex(index, keep.registry, true)
    }
  }
}
