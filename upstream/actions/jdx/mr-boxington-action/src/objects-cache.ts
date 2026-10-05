import {Buffer} from 'node:buffer'
import {lstat, mkdir, mkdtemp, opendir, readFile, realpath, rm, rmdir, statfs} from 'node:fs/promises'
import path from 'node:path'

export type ObjectsBundleForm = 'directory' | 'tar'

export interface ObjectsCachePaths {
  runnerTemp: string
  root: string
  store: string
  bundle: string
  form: ObjectsBundleForm
}

export interface TreeUsage {
  files: number
  directories: number
  symlinks: number
  uniqueInodes: number
  apparentBytes: string
  uniqueApparentBytes: string
  uniqueInodeAllocatedBytes: string
  hardlinkAliases: number
  entriesScanned: number
  complete: boolean
}

export interface MountUsage {
  identity: string
  roles: string[]
  freeBytes: string
  freeInodes: string
}

export interface ResourcePhase {
  schema: 1
  phase: string
  mounts: MountUsage[]
  mbxStore: TreeUsage | null
  mbxActions: TreeUsage | null
  mbxTargets: TreeUsage | null
  bundle: TreeUsage | null
  cargoTarget: (TreeUsage & {capturedAt: string}) | null
  accounting: string
}

const MAX_TREE_ENTRIES = 1_000_000
const SAMPLER_INTERVAL_MS = 250
const MAX_RECORDED_SAMPLE_FAILURES = 1_000
const MISSING = Symbol('missing')

function bundleName(form: ObjectsBundleForm): string {
  return form === 'directory' ? 'mbx-github-objects-bundle-v1' : 'mbx-github-objects-bundle-v1.tar'
}

function errorCode(error: unknown): string | undefined {
  return error && typeof error === 'object' && 'code' in error
    ? String((error as NodeJS.ErrnoException).code)
    : undefined
}

function sampleFailureReason(error: unknown): string {
  if (!error || (typeof error !== 'object' && typeof error !== 'function')) return 'unknown'
  try {
    const code = (error as {code?: unknown}).code
    if (typeof code === 'string' && /^[A-Za-z0-9_-]{1,32}$/.test(code)) return `code:${code}`
    const name = (error as {name?: unknown}).name
    if (typeof name === 'string' && /^[A-Za-z][A-Za-z0-9_]{0,31}$/.test(name)) return `type:${name}`
  } catch {
    // Diagnostic failures must never become action failures.
  }
  return 'unknown'
}

async function maybeLstat(value: string) {
  try {
    return await lstat(value, {bigint: true})
  } catch (error) {
    if (errorCode(error) === 'ENOENT') return MISSING
    throw error
  }
}

function isWithin(parent: string, child: string): boolean {
  const relative = path.relative(parent, child)
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))
}

export async function createObjectsCachePaths(
  runnerTempInput: string,
  form: ObjectsBundleForm
): Promise<ObjectsCachePaths> {
  if (!runnerTempInput || !path.isAbsolute(runnerTempInput)) {
    throw new Error('RUNNER_TEMP must be an absolute directory for isolated objects caching')
  }
  const tempStat = await lstat(runnerTempInput)
  if (tempStat.isSymbolicLink() || !tempStat.isDirectory()) {
    throw new Error('RUNNER_TEMP must be a real directory for isolated objects caching')
  }
  const runnerTemp = await realpath(runnerTempInput)
  const bundle = path.join(runnerTemp, bundleName(form))
  if (await maybeLstat(bundle) !== MISSING) {
    throw new Error('stable isolated objects bundle path already exists in RUNNER_TEMP')
  }
  const root = await mkdtemp(path.join(runnerTemp, 'mbx-github-objects-store-'))
  const paths: ObjectsCachePaths = {
    runnerTemp,
    root,
    store: path.join(root, 'store'),
    bundle,
    form
  }
  await mkdir(paths.store)
  await validateObjectsCachePaths(paths)
  return paths
}

export async function pathsFromObjectsCacheRoot(
  runnerTempInput: string,
  rootInput: string,
  form: ObjectsBundleForm
): Promise<ObjectsCachePaths> {
  if (!runnerTempInput || !path.isAbsolute(runnerTempInput)) {
    throw new Error('RUNNER_TEMP must be an absolute directory for isolated objects caching')
  }
  const runnerTemp = await realpath(runnerTempInput)
  const root = path.resolve(rootInput)
  const paths: ObjectsCachePaths = {
    runnerTemp,
    root,
    store: path.join(root, 'store'),
    bundle: path.join(runnerTemp, bundleName(form)),
    form
  }
  await validateObjectsCachePaths(paths)
  return paths
}

export function assertIsolatedObjectsActionStore(
  paths: ObjectsCachePaths,
  resolvedActionStore: string
): void {
  if (path.resolve(resolvedActionStore) !== path.join(paths.store, 'actions')) {
    throw new Error('mbx resolved its action store outside the private isolated cache store')
  }
}

export async function validateObjectsCachePaths(paths: ObjectsCachePaths): Promise<void> {
  const runnerTemp = path.resolve(paths.runnerTemp)
  const root = path.resolve(paths.root)
  const store = path.resolve(paths.store)
  const bundle = path.resolve(paths.bundle)
  const expectedBundle = path.join(runnerTemp, bundleName(paths.form))
  if (
    runnerTemp !== paths.runnerTemp ||
    root !== paths.root ||
    store !== paths.store ||
    bundle !== paths.bundle ||
    store !== path.join(root, 'store') ||
    bundle !== expectedBundle ||
    store === bundle ||
    isWithin(root, bundle) ||
    isWithin(bundle, root) ||
    path.dirname(root) !== runnerTemp ||
    path.dirname(bundle) !== runnerTemp ||
    !path.basename(root).startsWith('mbx-github-objects-store-')
  ) {
    throw new Error('isolated objects cache paths are not the exact private RUNNER_TEMP layout')
  }
  const [tempReal, rootStat, rootReal] = await Promise.all([
    realpath(runnerTemp),
    lstat(root),
    realpath(root)
  ])
  if (tempReal !== runnerTemp || rootStat.isSymbolicLink() || !rootStat.isDirectory() || rootReal !== root) {
    throw new Error('isolated objects cache root is not a canonical private directory')
  }
  for (const value of [store, bundle]) {
    const found = await maybeLstat(value)
    if (found !== MISSING && found.isSymbolicLink()) {
      throw new Error(`isolated objects cache path ${path.basename(value)} must not be a symlink`)
    }
    if (found !== MISSING) {
      const actual = await realpath(value)
      const expectedParent = value === store ? root : runnerTemp
      if (actual !== value || !isWithin(expectedParent, actual)) {
        throw new Error(`isolated objects cache path ${path.basename(value)} is not canonical`)
      }
    }
  }
}

export async function measureTree(
  target: string,
  rejectSymlinks: boolean,
  entryLimit = MAX_TREE_ENTRIES
): Promise<TreeUsage> {
  if (!Number.isSafeInteger(entryLimit) || entryLimit < 1) {
    throw new Error('tree measurement entry limit must be a positive safe integer')
  }
  const rootStat = await lstat(target, {bigint: true})
  if (rootStat.isSymbolicLink()) throw new Error(`cache path ${path.basename(target)} is a symlink`)
  const pending = [target]
  const seenInodes = new Set<string>()
  let files = 0
  let directories = 0
  let symlinks = 0
  let entriesScanned = 0
  let apparentBytes = 0n
  let uniqueApparentBytes = 0n
  let uniqueInodeAllocatedBytes = 0n
  let hardlinkAliases = 0
  let complete = true

  while (pending.length > 0) {
    if (entriesScanned >= entryLimit) {
      complete = false
      if (rejectSymlinks) {
        throw new Error(`exported cache bundle exceeds the ${entryLimit} entry validation limit`)
      }
      break
    }
    const current = pending.pop()!
    const info = await lstat(current, {bigint: true})
    entriesScanned++
    const inode = `${info.dev}:${info.ino}`
    const firstInode = !seenInodes.has(inode)
    if (firstInode) {
      seenInodes.add(inode)
      uniqueInodeAllocatedBytes += info.blocks * 512n
    } else if (info.isFile()) {
      hardlinkAliases++
    }
    if (info.isSymbolicLink()) {
      symlinks++
      if (rejectSymlinks) {
        throw new Error(`exported cache bundle contains a symlink at ${path.basename(current)}`)
      }
      continue
    }
    if (info.isDirectory()) {
      directories++
      const children = await opendir(current)
      for await (const child of children) {
        if (entriesScanned + pending.length >= entryLimit) {
          complete = false
          break
        }
        pending.push(path.join(current, child.name))
      }
      if (complete === false && rejectSymlinks) {
        throw new Error(`exported cache bundle exceeds the ${entryLimit} entry validation limit`)
      }
    } else if (info.isFile()) {
      files++
      apparentBytes += info.size
      if (firstInode) uniqueApparentBytes += info.size
    } else if (rejectSymlinks) {
      throw new Error(`exported cache bundle contains a non-file entry at ${path.basename(current)}`)
    }
  }

  return {
    files,
    directories,
    symlinks,
    uniqueInodes: seenInodes.size,
    apparentBytes: apparentBytes.toString(),
    uniqueApparentBytes: uniqueApparentBytes.toString(),
    uniqueInodeAllocatedBytes: uniqueInodeAllocatedBytes.toString(),
    hardlinkAliases,
    entriesScanned,
    complete
  }
}

export async function validateObjectsBundle(paths: ObjectsCachePaths): Promise<TreeUsage> {
  await validateObjectsCachePaths(paths)
  const info = await maybeLstat(paths.bundle)
  if (info === MISSING) throw new Error('mbx cache export did not create its private bundle')
  if (paths.form === 'directory' ? !info.isDirectory() : !info.isFile()) {
    throw new Error(`mbx cache export created the wrong bundle type for ${paths.form} format`)
  }
  const usage = await measureTree(paths.bundle, true)
  if (usage.files === 0 || usage.apparentBytes === '0') {
    throw new Error('mbx cache export created an empty bundle')
  }
  return usage
}

export async function assertObjectsBundleAbsent(paths: ObjectsCachePaths): Promise<void> {
  await validateObjectsCachePaths(paths)
  if (await maybeLstat(paths.bundle) !== MISSING) {
    throw new Error('private objects bundle exists when no cache restore was reported')
  }
}

export async function importObjectsBundle(
  paths: ObjectsCachePaths,
  importBundle: (bundlePath: string) => Promise<void>
): Promise<void> {
  await validateObjectsBundle(paths)
  await importBundle(paths.bundle)
  // MBX 1.22 removes directory bundles after a successful import. The tar
  // compatibility form is still action-owned, so both forms use the same
  // post-import cleanup after MBX returns successfully.
  const info = await maybeLstat(paths.bundle)
  if (info !== MISSING) {
    await validateObjectsCachePaths(paths)
    await rm(paths.bundle, {recursive: true, force: false})
  }
}

export async function removeObjectsStore(paths: ObjectsCachePaths): Promise<void> {
  await validateObjectsCachePaths(paths)
  const info = await maybeLstat(paths.store)
  if (info === MISSING) return
  if (!info.isDirectory()) throw new Error('isolated mbx store is not a directory')
  await rm(paths.store, {recursive: true, force: false})
}

export async function removeObjectsBundle(paths: ObjectsCachePaths): Promise<void> {
  await validateObjectsCachePaths(paths)
  const info = await maybeLstat(paths.bundle)
  if (info === MISSING) return
  await rm(paths.bundle, {recursive: true, force: false})
}

export async function cleanupIsolatedObjectsCachePost(paths: ObjectsCachePaths): Promise<void> {
  await validateObjectsCachePaths(paths)
  await removeObjectsBundle(paths)
  await removeObjectsStore(paths)
  await validateObjectsCachePaths(paths)
  const rootEntries = await opendir(paths.root)
  try {
    const firstEntry = await rootEntries.read()
    if (firstEntry) {
      throw new Error('isolated mbx cache root contains an unexpected entry after store cleanup')
    }
  } finally {
    await rootEntries.close()
  }
  await rmdir(paths.root)
}

export async function withIsolatedObjectsPostCleanup<T>(
  paths: ObjectsCachePaths,
  operation: () => Promise<T>,
  warn: (message: string) => void
): Promise<T> {
  let operationSucceeded = false
  try {
    const result = await operation()
    operationSucceeded = true
    return result
  } finally {
    try {
      await cleanupIsolatedObjectsCachePost(paths)
    } catch (cleanupError) {
      if (operationSucceeded) throw cleanupError
      const code = errorCode(cleanupError)
      try {
        warn(
          `Could not remove isolated mbx cache after post failure${code ? ` (${code})` : ''}; ` +
            'the original post error is preserved'
        )
      } catch {
        // A secondary logging failure must not replace the original post error.
      }
    }
  }
}

async function canonicalExistingDirectory(
  target: string,
  followTargetRootSymlink = false
): Promise<string | undefined> {
  const targetRoot = path.resolve(target)
  let current = targetRoot
  while (true) {
    const found = await maybeLstat(current)
    if (found !== MISSING) {
      if (found.isSymbolicLink()) {
        if (followTargetRootSymlink && current === targetRoot) {
          try {
            const resolved = await realpath(current)
            if ((await lstat(resolved)).isDirectory()) return resolved
          } catch (error) {
            if (errorCode(error) !== 'ENOENT') throw error
          }
        }
        return undefined
      }
      if (!found.isDirectory()) current = path.dirname(current)
      else return realpath(current)
    } else {
      const parent = path.dirname(current)
      if (parent === current) return undefined
      current = parent
    }
  }
}

async function linuxMountIdentity(target: string): Promise<string | undefined> {
  if (process.platform !== 'linux') return undefined
  try {
    const mountInfo = await readFile('/proc/self/mountinfo', 'utf8')
    const unescape = (value: string) =>
      value.replace(/\\040/g, ' ').replace(/\\011/g, '\t').replace(/\\012/g, '\n').replace(/\\134/g, '\\')
    const mounts = mountInfo.split('\n').flatMap(line => {
      if (!line) return []
      const [left, right] = line.split(' - ')
      const fields = left?.split(' ') ?? []
      const tail = right?.split(' ') ?? []
      const mountPoint = unescape(fields[4] ?? '')
      if (!mountPoint || !fields[0] || !fields[2] || !tail[0]) return []
      return [{id: fields[0], device: fields[2], mountPoint, type: tail[0]}]
    })
    const match = mounts
      .filter(item => target === item.mountPoint || target.startsWith(`${item.mountPoint.replace(/\/$/, '')}/`))
      .sort((a, b) => b.mountPoint.length - a.mountPoint.length)[0]
    return match ? `${match.id}:${match.device}:${match.type}` : undefined
  } catch {
    return undefined
  }
}

async function mountUsage(
  target: string,
  role: string,
  followTargetRootSymlink = false
): Promise<MountUsage | undefined> {
  const existingDirectory = await canonicalExistingDirectory(target, followTargetRootSymlink)
  if (!existingDirectory) return undefined
  const [fsStats, targetStats, identity] = await Promise.all([
    statfs(existingDirectory, {bigint: true}),
    lstat(existingDirectory, {bigint: true}),
    linuxMountIdentity(existingDirectory)
  ])
  const key = identity ?? `${targetStats.dev}:${fsStats.type}`
  return {
    identity: key,
    roles: [role],
    freeBytes: (fsStats.bavail * fsStats.bsize).toString(),
    freeInodes: fsStats.ffree.toString()
  }
}

async function mountUsages(paths: ObjectsCachePaths, cargoTarget: string): Promise<MountUsage[]> {
  const samples = await Promise.all([
    mountUsage(paths.runnerTemp, 'runnerTemp'),
    mountUsage(paths.root, 'objectsCache'),
    mountUsage(paths.store, 'mbxStore'),
    mountUsage(paths.bundle, 'bundle'),
    mountUsage(cargoTarget, 'cargoTarget', true)
  ])
  const unique = new Map<string, MountUsage>()
  for (const sample of samples) {
    if (!sample) continue
    const existing = unique.get(sample.identity)
    if (existing) {
      existing.roles.push(...sample.roles)
      if (BigInt(sample.freeBytes) < BigInt(existing.freeBytes)) existing.freeBytes = sample.freeBytes
      if (BigInt(sample.freeInodes) < BigInt(existing.freeInodes)) existing.freeInodes = sample.freeInodes
    } else unique.set(sample.identity, sample)
  }
  return [...unique.values()]
}

async function treeUsageIfPresent(target: string, rejectSymlinks = false): Promise<TreeUsage | null> {
  const found = await maybeLstat(target)
  if (found === MISSING) return null
  return measureTree(target, rejectSymlinks)
}

async function cargoTargetUsageIfPresent(target: string): Promise<TreeUsage | null> {
  const found = await maybeLstat(target)
  if (found === MISSING) return null
  try {
    // Cargo accepts a symlinked target directory. Follow only this
    // user-controlled telemetry root; action-owned bundle/store roots remain
    // subject to strict canonical-path checks.
    const measuredPath = found.isSymbolicLink() ? await realpath(target) : target
    return await measureTree(measuredPath, false)
  } catch (error) {
    if (errorCode(error) === 'ENOENT') return null
    throw error
  }
}

export async function reportObjectsResourcePhase(
  paths: ObjectsCachePaths,
  phase: string,
  cargoTarget: string,
  emit: (message: string) => void,
  cachedCargoTarget?: TreeUsage & {capturedAt: string}
): Promise<(TreeUsage & {capturedAt: string}) | null> {
  await validateObjectsCachePaths(paths)
  const [mounts, storeUsage, bundleUsage] = await Promise.all([
    mountUsages(paths, cargoTarget),
    treeUsageIfPresent(paths.store),
    treeUsageIfPresent(paths.bundle, true)
  ])
  const [actionsUsage, targetsUsage] = await Promise.all([
    treeUsageIfPresent(path.join(paths.store, 'actions')),
    treeUsageIfPresent(path.join(paths.store, 'targets'))
  ])
  const targetUsage = cachedCargoTarget ?? (await cargoTargetUsageIfPresent(cargoTarget))
  const cargo = targetUsage
    ? {...targetUsage, capturedAt: cachedCargoTarget?.capturedAt ?? phase}
    : null
  const record: ResourcePhase = {
    schema: 1,
    phase,
    mounts,
    mbxStore: storeUsage,
    mbxActions: actionsUsage,
    mbxTargets: targetsUsage,
    bundle: bundleUsage,
    cargoTarget: cargo,
    accounting:
      'apparent bytes count regular-file paths; allocated bytes sum st_blocks for unique inodes; ' +
      'hardlinks are deduped, reflink/shared extents cannot be deduped with Node stat; ' +
      'mbxStore, mbxActions, and mbxTargets overlap and must not be summed'
  }
  emit(`Objects cache resources ${JSON.stringify(record)}`)
  return cargo
}

export interface SampledArchiveUsage {
  observed: boolean
  samples: number
  maxObservedApparentBytes: string | null
  maxObservedAllocatedBytes: string | null
  scanComplete: boolean
  scanLimit: number
  scanIncompleteReason: string | null
}

export interface CacheArchiveFile {
  path: string
  size: bigint
  allocated: bigint
  mtimeMs: number
}

export interface CacheArchiveScan {
  files: CacheArchiveFile[]
  complete: boolean
  incompleteReason?: string
}

async function cacheArchiveFiles(runnerTemp: string): Promise<CacheArchiveScan> {
  const archives: Array<{path: string; size: bigint; allocated: bigint; mtimeMs: number}> = []
  let children
  try {
    children = await opendir(runnerTemp)
  } catch {
    return {files: archives, complete: false, incompleteReason: 'could-not-open-runner-temp'}
  }

  let entriesScanned = 0
  try {
    for await (const child of children) {
      if (entriesScanned >= MAX_TREE_ENTRIES) {
        return {files: archives, complete: false, incompleteReason: 'runner-temp-entry-limit'}
      }
      entriesScanned++
      if (!child.isDirectory()) continue
      const folder = path.join(runnerTemp, child.name)
      const folderStat = await maybeLstat(folder)
      if (folderStat === MISSING || !folderStat.isDirectory() || folderStat.isSymbolicLink()) continue
      for (const name of ['cache.tgz', 'cache.tzst', 'cache.tar']) {
        const file = path.join(folder, name)
        const info = await maybeLstat(file)
        if (info !== MISSING && info.isFile()) {
          if (archives.length >= MAX_TREE_ENTRIES) {
            return {files: archives, complete: false, incompleteReason: 'runner-temp-archive-limit'}
          }
          archives.push({
            path: file,
            size: info.size,
            allocated: info.blocks * 512n,
            mtimeMs: Number(info.mtimeNs / 1_000_000n)
          })
        }
      }
    }
  } catch {
    return {files: archives, complete: false, incompleteReason: 'runner-temp-scan-failed'}
  }
  return {files: archives, complete: true}
}

export interface ObjectsResourceSamplerDependencies {
  mountUsages?: (paths: ObjectsCachePaths, cargoTarget: string) => Promise<MountUsage[]>
  cacheArchiveFiles?: (runnerTemp: string) => Promise<CacheArchiveScan>
  intervalMs?: number
}

export async function withObjectsResourceSampler<T>(
  paths: ObjectsCachePaths,
  cargoTarget: string,
  phase: string,
  operation: () => Promise<T>,
  emit: (message: string) => void,
  dependencies: ObjectsResourceSamplerDependencies = {}
): Promise<{result: T; archives: SampledArchiveUsage}> {
  const minima = new Map<string, MountUsage>()
  const readMountUsages = dependencies.mountUsages ?? mountUsages
  const scanArchives = dependencies.cacheArchiveFiles ?? cacheArchiveFiles
  const sampleIntervalMs = dependencies.intervalMs ?? SAMPLER_INTERVAL_MS
  let sampleFailureCount = 0
  let sampleFailureCountCapped = false
  let firstSampleFailureReason: string | null = null
  const recordSampleFailure = (error: unknown) => {
    if (sampleFailureCount < MAX_RECORDED_SAMPLE_FAILURES) sampleFailureCount++
    else sampleFailureCountCapped = true
    firstSampleFailureReason ??= sampleFailureReason(error)
  }
  let baselineScan: CacheArchiveScan
  try {
    baselineScan = await scanArchives(paths.runnerTemp)
  } catch (error) {
    recordSampleFailure(error)
    baselineScan = {files: [], complete: false, incompleteReason: 'baseline-scan-failed'}
  }
  const archive: SampledArchiveUsage = {
    observed: false,
    samples: 0,
    maxObservedApparentBytes: null,
    maxObservedAllocatedBytes: null,
    scanComplete: baselineScan.complete,
    scanLimit: MAX_TREE_ENTRIES,
    scanIncompleteReason: baselineScan.incompleteReason ?? null
  }
  let inFlightSample: Promise<void> | undefined
  let sampleCount = 0
  const startTime = Date.now()
  let previousSampleAt: number | undefined
  let maxObservedIntervalMs = 0
  const baseline = new Map(
    baselineScan.files.map(value => [value.path, `${value.size}:${value.mtimeMs}`])
  )
  const sample = async () => {
    if (inFlightSample) return inFlightSample
    const pending = (async () => {
      sampleCount++
      const sampledAt = Date.now()
      if (previousSampleAt !== undefined) {
        maxObservedIntervalMs = Math.max(maxObservedIntervalMs, sampledAt - previousSampleAt)
      }
      previousSampleAt = sampledAt
      for (const usage of await readMountUsages(paths, cargoTarget)) {
        const current = minima.get(usage.identity)
        if (!current) minima.set(usage.identity, {...usage, roles: [...usage.roles]})
        else {
          current.roles = [...new Set([...current.roles, ...usage.roles])]
          if (BigInt(usage.freeBytes) < BigInt(current.freeBytes)) current.freeBytes = usage.freeBytes
          if (BigInt(usage.freeInodes) < BigInt(current.freeInodes)) current.freeInodes = usage.freeInodes
        }
      }
      const staged = await scanArchives(paths.runnerTemp)
      if (!staged.complete) {
        archive.scanComplete = false
        archive.scanIncompleteReason ??= staged.incompleteReason ?? 'runner-temp-scan-incomplete'
      }
      for (const candidate of staged.files) {
        if (baseline.get(candidate.path) === `${candidate.size}:${candidate.mtimeMs}`) continue
        archive.observed = true
        archive.samples++
        const apparent = BigInt(archive.maxObservedApparentBytes ?? '0')
        const allocated = BigInt(archive.maxObservedAllocatedBytes ?? '0')
        if (candidate.size > apparent) archive.maxObservedApparentBytes = candidate.size.toString()
        if (candidate.allocated > allocated) archive.maxObservedAllocatedBytes = candidate.allocated.toString()
      }
    })()
    const handled = pending.catch(error => {
      recordSampleFailure(error)
      archive.scanComplete = false
      archive.scanIncompleteReason ??= 'resource-sample-failed'
    })
    inFlightSample = handled
    try {
      await handled
    } finally {
      if (inFlightSample === handled) inFlightSample = undefined
    }
  }
  await sample()
  const timer = setInterval(() => {
    if (!inFlightSample) void sample()
  }, sampleIntervalMs)
  let result!: T
  let thrown: unknown
  let operationThrew = false
  try {
    result = await operation()
  } catch (error) {
    operationThrew = true
    thrown = error
  } finally {
    clearInterval(timer)
    if (inFlightSample) {
      try {
        await inFlightSample
      } catch (error) {
        recordSampleFailure(error)
      }
    }
    await sample()
    try {
      emit(
        `Objects cache sample ${JSON.stringify({
          schema: 1,
          phase,
          sampleIntervalMs,
          maxObservedIntervalMs,
          durationMs: Date.now() - startTime,
          sampleCount,
          samplingFailures: {
            count: sampleFailureCount,
            countCapped: sampleFailureCountCapped,
            firstReason: firstSampleFailureReason
          },
          mounts: [...minima.values()],
          cacheArchiveStaging: archive,
          mbxExportStagingNote:
            phase === 'bundle-export'
              ? 'MBX writes a sibling temporary tree and atomically publishes it as bundle; bundle bytes below measure the published same tree, while statfs records the lowest available bytes and inodes observed at sampling intervals, not instantaneous peak pressure'
              : undefined,
          cacheArchiveNote: archive.observed
            ? 'local actions/cache archive observed separately from saveCache result'
            : 'actions/cache archive was not observed during sampling; no upload size inferred'
        })}`
      )
    } catch {
      // Resource telemetry must not replace the operation result or error.
    }
  }
  if (operationThrew) throw thrown
  return {result, archives: archive}
}

export type CacheSaveFailure =
  | 'service-reservation'
  | 'service-5xx'
  | 'network-transport'
  | 'local-storage'
  | 'unknown'

// Classify only terminal @actions/cache workflow output. Its intermediate SDK
// warnings and Twirp retry lines can report a failure that later succeeds.
export function classifyCacheSaveFailure(output: string): CacheSaveFailure {
  if (
    /::(?:warning|error)::Failed to save:[^\n]*\b(?:ENOSPC|no space left on device|disk quota exceeded)\b/i.test(
      output
    )
  ) {
    return 'local-storage'
  }
  if (
    /::warning::Failed to save: (?:reserveCache|uploadChunk \([^)]*\)|commitCache) failed: Cache service responded with 5\d\d/i.test(
      output
    ) ||
    /::error::Failed to save: (?:reserveCache|uploadChunk \([^)]*\)|commitCache) failed: Cache service responded with 5\d\d/i.test(
      output
    ) ||
    /::(?:warning|error)::Failed to save: Failed to FinalizeCacheEntryUpload:[^\n]*Failed request: \(5\d\d\)/i.test(
      output
    ) ||
    /::(?:warning|error)::Failed to save: uploadCacheArchiveSDK: upload failed with status code 5\d\d\r?\n/i.test(
      output
    )
  ) {
    return 'service-5xx'
  }
  if (
    /(?:^|\r?\n)Failed to save: Unable to reserve cache with key [^\r\n]*, another job may be creating this cache\.(?: More details: [^\r\n]*)?\r?(?:\n|$)/i.test(
      output
    ) ||
    /::warning::(?:Failed to save: )?Unable to finalize cache with key [^\r\n]*, another job may be finalizing this cache\.\r?\n/i.test(
      output
    )
  ) {
    return 'service-reservation'
  }
  if (
    /::(?:warning|error)::Failed to save: (?:reserveCache|uploadChunk \([^)]*\)|commitCache) failed:[^\n]*(?:Request timeout|\b(?:ETIMEDOUT|ECONNRESET|ENOTFOUND|EAI_AGAIN|ECONNREFUSED|ECONNABORTED|EHOSTUNREACH)\b)/i.test(
      output
    ) ||
    /::(?:warning|error)::Failed to save: Failed to FinalizeCacheEntryUpload:[^\n]*(?:Request timeout|\b(?:ETIMEDOUT|ECONNRESET|ENOTFOUND|EAI_AGAIN|ECONNREFUSED|ECONNABORTED|EHOSTUNREACH)\b)/i.test(
      output
    ) ||
    /::(?:warning|error)::Failed to save:[^\n]*(?:Request timeout|\b(?:ETIMEDOUT|ECONNRESET|ENOTFOUND|EAI_AGAIN|ECONNREFUSED|ECONNABORTED|EHOSTUNREACH)\b)/i.test(
      output
    )
  ) {
    return 'network-transport'
  }
  return 'unknown'
}

export async function withBoundedActionOutput<T>(
  operation: () => Promise<T>
): Promise<{result: T; output: string}> {
  const chunks: string[] = []
  let length = 0
  const capture = (chunk: unknown) => {
    const value = Buffer.isBuffer(chunk) ? chunk.toString('utf8') : String(chunk)
    chunks.push(value)
    length += value.length
    while (length > 16_384 && chunks.length > 0) {
      const excess = length - 16_384
      const first = chunks[0]!
      if (first.length <= excess) {
        chunks.shift()
        length -= first.length
      } else {
        chunks[0] = first.slice(excess)
        length -= excess
      }
    }
  }
  const stdout = process.stdout.write
  const stderr = process.stderr.write
  process.stdout.write = function (chunk: never, ...args: never[]) {
    capture(chunk)
    return (stdout as (...values: never[]) => boolean).call(process.stdout, chunk, ...args)
  } as typeof process.stdout.write
  process.stderr.write = function (chunk: never, ...args: never[]) {
    capture(chunk)
    return (stderr as (...values: never[]) => boolean).call(process.stderr, chunk, ...args)
  } as typeof process.stderr.write
  try {
    return {result: await operation(), output: chunks.join('')}
  } finally {
    process.stdout.write = stdout
    process.stderr.write = stderr
  }
}

export async function saveIsolatedObjectsBundle(options: {
  paths: ObjectsCachePaths
  primaryKey: string
  saveEligible: boolean
  exactHit: boolean
  cargoTarget: string
  exportBundle: (bundlePath: string) => Promise<{exitCode: number; output: string}>
  isEmptyExport: (output: string) => boolean
  saveCache: (paths: string[], primaryKey: string) => Promise<number>
  emit: (message: string) => void
  warn: (message: string) => void
}): Promise<'ineligible' | 'exact-hit' | 'empty' | 'saved' | 'save-unavailable'> {
  await validateObjectsCachePaths(options.paths)
  await reportObjectsResourcePhase(
    options.paths,
    'after-build-before-export',
    options.cargoTarget,
    options.emit
  )
  if (!options.saveEligible) return 'ineligible'
  if (options.exactHit) return 'exact-hit'
  if (await maybeLstat(options.paths.bundle) !== MISSING) {
    throw new Error('private objects bundle already exists before export')
  }
  const exported = await withObjectsResourceSampler(
    options.paths,
    options.cargoTarget,
    'bundle-export',
    () => options.exportBundle(options.paths.bundle),
    options.emit
  )
  if (exported.result.exitCode !== 0) {
    if (options.isEmptyExport(exported.result.output)) {
      await reportObjectsResourcePhase(
        options.paths,
        'after-bundle-export-empty',
        options.cargoTarget,
        options.emit
      )
      await assertObjectsBundleAbsent(options.paths)
      options.emit('No completed mbx build was recorded; not saving an empty cache')
      return 'empty'
    }
    throw new Error(`mbx cache export exited with code ${exported.result.exitCode}`)
  }
  await validateObjectsCachePaths(options.paths)
  const bundleUsage = await validateObjectsBundle(options.paths)
  const cargoUsage = await reportObjectsResourcePhase(
    options.paths,
    'after-bundle-export',
    options.cargoTarget,
    options.emit
  )
  await removeObjectsStore(options.paths)
  await reportObjectsResourcePhase(
    options.paths,
    'after-store-removal',
    options.cargoTarget,
    options.emit,
    cargoUsage ? {...cargoUsage, capturedAt: 'after-bundle-export'} : undefined
  )
  options.emit(
    `Objects cache local staging ${JSON.stringify({
      bundleApparentBytes: bundleUsage.apparentBytes,
      bundleAllocatedBytes: bundleUsage.uniqueInodeAllocatedBytes,
      bundleFiles: bundleUsage.files,
      note: 'bundle is local input to actions/cache; upload success is reported separately'
    })}`
  )
  const sampled = await withObjectsResourceSampler(
    options.paths,
    options.cargoTarget,
    'actions-cache-save',
    () => withBoundedActionOutput(() => options.saveCache([options.paths.bundle], options.primaryKey)),
    options.emit
  )
  const {result: saveResult, output} = sampled.result
  await reportObjectsResourcePhase(
    options.paths,
    'after-cache-save',
    options.cargoTarget,
    options.emit,
    cargoUsage ? {...cargoUsage, capturedAt: 'after-bundle-export'} : undefined
  )
  const failure = classifyCacheSaveFailure(output)
  if (
    failure === 'service-reservation' ||
    failure === 'service-5xx' ||
    failure === 'network-transport'
  ) {
    options.warn(`GitHub cache save skipped after classified ${failure}`)
    await removeObjectsBundle(options.paths)
    return 'save-unavailable'
  }
  if (failure === 'local-storage') {
    throw new Error('actions/cache could not stage the local archive because runner storage is full')
  }
  const v2CacheService =
    Boolean(process.env.ACTIONS_CACHE_SERVICE_V2) &&
    !(() => {
      try {
        const host = new URL(process.env.GITHUB_SERVER_URL || 'https://github.com').hostname.toUpperCase()
        return host !== 'GITHUB.COM' && !host.endsWith('.GHE.COM') && !host.endsWith('.LOCALHOST')
      } catch {
        return true
      }
    })()
  const hasSuccessEvidence =
    output.includes('Cache saved successfully') || (v2CacheService && saveResult >= 0)
  if (saveResult < 0 || !hasSuccessEvidence) {
    throw new Error('actions/cache did not provide evidence that the objects bundle was saved')
  }
  options.emit(
    `Saved mbx objects bundle; actions/cache confirmed upload/finalization (service ${v2CacheService ? 'v2' : 'v1'})`
  )
  await removeObjectsBundle(options.paths)
  return 'saved'
}
