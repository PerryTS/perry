import * as cache from '@actions/cache'
import * as core from '@actions/core'
import * as exec from '@actions/exec'
import {context} from '@actions/github'
import * as tc from '@actions/tool-cache'
import {createHash, randomUUID} from 'node:crypto'
import {access, chmod, copyFile, mkdir, readFile, stat} from 'node:fs/promises'
import {constants} from 'node:fs'
import {homedir} from 'node:os'
import path from 'node:path'
import {
  aliasedInput,
  type BundleForm,
  cacheLinksValue,
  cacheRevision,
  canReuseCachedMbx,
  callingCard,
  type CallingCardRow,
  cargoTargetDirectory,
  effectiveRestoreKeys,
  generatedKey,
  generatedRestoreKey,
  githubCacheGeneration,
  githubObjectGcDefault,
  githubApiHeaders,
  githubTokenValue,
  isEmptyExport,
  mbxReleaseToInstall,
  normalizedVersion,
  parseBackend,
  parseGithubCacheMode,
  parsedMbxVersion,
  primaryCacheKey,
  pullRequestRestoreKey,
  type PullRequestRepositories,
  remoteExports,
  remoteStatus,
  type RemoteStatus,
  requireGithubCacheRuntime,
  releaseTarget,
  rustcIdentityArgs,
  isSameRepositoryPullRequest,
  savePolicy,
  supportsDirectoryBundle,
  toolchainSegment,
  verifiedReleaseAsset,
  type GithubRelease,
  type VerifiedReleaseAsset
} from './lib.js'
import {
  assertObjectsBundleAbsent,
  assertIsolatedObjectsActionStore,
  createObjectsCachePaths,
  importObjectsBundle,
  pathsFromObjectsCacheRoot,
  reportObjectsResourcePhase,
  saveIsolatedObjectsBundle,
  withIsolatedObjectsPostCleanup,
  type ObjectsCachePaths
} from './objects-cache.js'
import {
  dehydrateMbxShimBinaries,
  hasReusableCargoTarget,
  hydrateMbxShimBinaries,
  pruneCargoTargetCache
} from './target-cache.js'

const POST_STATE = 'mbx-post'
const CACHE_KEY_STATE = 'mbx-cache-key'
const CACHE_HIT_STATE = 'mbx-cache-hit'
const CACHE_ARCHIVE_STATE = 'mbx-cache-archive'
const CACHE_EXPORT_GROUP_STATE = 'mbx-cache-export-group'
const CACHE_PATHS_STATE = 'mbx-cache-paths'
const CACHE_BUNDLE_FORM_STATE = 'mbx-cache-bundle-form'
const CACHE_ISOLATION_ROOT_STATE = 'mbx-cache-isolation-root'
const CARGO_WORKSPACE_STATE = 'mbx-cargo-workspace'
const MBX_STATE = 'mbx-bin'
const CACHE_ARCHIVE_NAME = 'github-actions-cache-v1.tar'
// A directory rather than a tar. `actions/cache` archives whatever path it is
// given, so a tar inside its archive means every byte is written twice on
// restore: once when it unpacks, and again when `mbx cache import` does.
const CACHE_BUNDLE_NAME = 'github-actions-cache-v1'
const TARGET_TOOL_DIRECTORY = 'mbx-target-tool'

interface MbxInstallation {
  bin: string
  version: string
}

async function leaveCallingCard(note: string, rows: CallingCardRow[]): Promise<void> {
  try {
    await core.summary
      .addDetails(
        '📦 <strong>Mr Boxington inspected the premises.</strong>',
        callingCard(note, rows)
      )
      .write()
  } catch (error) {
    core.debug(`Could not write Mr Boxington's run summary: ${String(error)}`)
  }
}

async function capture(command: string, args: string[], cwd?: string): Promise<string> {
  let output = ''
  const exitCode = await exec.exec(command, args, {
    cwd,
    silent: true,
    listeners: {stdout: data => (output += data.toString())}
  })
  if (exitCode !== 0) throw new Error(`${command} exited with code ${exitCode}`)
  return output.trim()
}

async function isDirectory(directory: string): Promise<boolean> {
  try {
    return (await stat(directory)).isDirectory()
  } catch {
    return false
  }
}

/**
 * The verbose rustc identity, or null when the toolchain cannot be probed.
 *
 * `toolchain` is the empty string unless the caller named one, in which case
 * that toolchain is asked rather than whichever one `rustc` on `PATH` resolves
 * to.
 */
async function rustcIdentity(toolchain: string): Promise<string | null> {
  try {
    return await capture('rustc', rustcIdentityArgs(toolchain))
  } catch (error) {
    core.debug(`rustc identity probe failed: ${String(error)}`)
    return null
  }
}

async function resolveRelease(
  requested: string,
  archiveName: string,
  githubToken: string
): Promise<VerifiedReleaseAsset> {
  const endpoint =
    requested === 'latest'
      ? 'https://api.github.com/repos/jdx/mr-boxington/releases/latest'
      : `https://api.github.com/repos/jdx/mr-boxington/releases/tags/v${encodeURIComponent(requested)}`
  const response = await fetch(endpoint, {
    headers: githubApiHeaders(githubToken),
    redirect: 'error'
  })
  if (!response.ok) {
    throw new Error(`could not resolve mbx ${requested}: GitHub returned ${response.status}`)
  }
  return verifiedReleaseAsset((await response.json()) as GithubRelease, requested, archiveName)
}

async function installMbx(
  requested: string,
  githubToken: string
): Promise<MbxInstallation> {
  const requestedVersion = normalizedVersion(requested)
  const target = releaseTarget(process.platform, process.arch)
  const extension = process.platform === 'win32' ? 'zip' : 'tar.gz'
  const archiveName = `mbx-${target}.${extension}`
  const {version, sha256} = await resolveRelease(requestedVersion, archiveName, githubToken)
  const toolName = `mbx-${sha256}`
  const found = tc.find(toolName, version)
  if (found) {
    core.addPath(found)
    return {bin: path.join(found, process.platform === 'win32' ? 'mbx.exe' : 'mbx'), version}
  }

  const base = `https://github.com/jdx/mr-boxington/releases/download/v${version}`
  const archive = await tc.downloadTool(`${base}/${archiveName}`)
  const actual = createHash('sha256').update(await readFile(archive)).digest('hex')
  if (actual !== sha256) throw new Error(`checksum mismatch for ${archiveName}`)

  const extracted =
    process.platform === 'win32' ? await tc.extractZip(archive) : await tc.extractTar(archive)
  const extractedBin = path.join(extracted, process.platform === 'win32' ? 'mbx.exe' : 'mbx')
  if (process.platform !== 'win32') await chmod(extractedBin, 0o755)
  const rawVersion = await capture(extractedBin, ['--version'])
  const installedVersion = parsedMbxVersion(rawVersion)
  if (!installedVersion) throw new Error(`could not parse mbx version from ${JSON.stringify(rawVersion)}`)
  if (installedVersion !== version) {
    throw new Error(`mbx archive for ${version} contains version ${installedVersion}`)
  }
  const toolDir = await tc.cacheDir(extracted, toolName, installedVersion)
  core.addPath(toolDir)
  return {
    bin: path.join(toolDir, process.platform === 'win32' ? 'mbx.exe' : 'mbx'),
    version: installedVersion
  }
}

async function mbxOnPath(): Promise<MbxInstallation | undefined> {
  try {
    const names =
      process.platform === 'win32'
        ? (process.env.PATHEXT || '.EXE;.CMD;.BAT;.COM').split(';').map(extension => `mbx${extension}`)
        : ['mbx']
    let bin = ''
    for (const directory of (process.env.PATH || '').split(path.delimiter)) {
      for (const name of names) {
        const candidate = path.resolve(directory, name)
        try {
          await access(candidate, constants.X_OK)
          bin = candidate
          break
        } catch {}
      }
      if (bin) break
    }
    if (!bin) throw new Error('mbx was not found on PATH')
    const rawVersion = await capture(bin, ['--version'])
    const version = parsedMbxVersion(rawVersion)
    if (!version) throw new Error(`could not parse mbx version from ${JSON.stringify(rawVersion)}`)
    return {bin, version}
  } catch (error) {
    core.debug(`mbx PATH probe failed: ${String(error)}`)
    return undefined
  }
}

async function setupMbx(
  requested: string,
  githubToken: string,
  cachedDirectory = ''
): Promise<MbxInstallation> {
  const found = requested ? undefined : await mbxOnPath()
  const release = mbxReleaseToInstall(requested, Boolean(found))
  if (!release && found) {
    core.info(`Using mbx ${found.version} from PATH`)
    return found
  }
  if (cachedDirectory && release && release !== 'latest') {
    const cached = path.join(cachedDirectory, process.platform === 'win32' ? 'mbx.exe' : 'mbx')
    try {
      await access(cached, constants.X_OK)
      const version = parsedMbxVersion(await capture(cached, ['--version']))
      if (version && canReuseCachedMbx(release, version)) {
        core.info(`Using mbx ${version} from the restored target cache`)
        core.addPath(cachedDirectory)
        return {bin: cached, version}
      }
      core.debug(`Ignoring cached mbx ${version ?? 'with an unknown version'}; ${release} was requested`)
    } catch (error) {
      core.debug(`Cached mbx probe failed: ${String(error)}`)
    }
  }
  return installMbx(release ?? 'latest', githubToken)
}

async function stageTargetCacheMbx(
  installed: MbxInstallation,
  directory: string
): Promise<MbxInstallation> {
  const bin = path.join(directory, process.platform === 'win32' ? 'mbx.exe' : 'mbx')
  if (path.resolve(installed.bin) !== path.resolve(bin)) {
    await mkdir(directory, {recursive: true})
    await copyFile(installed.bin, bin)
    if (process.platform !== 'win32') await chmod(bin, 0o755)
  }
  core.addPath(directory)
  return {...installed, bin}
}

/**
 * Export the remote settings the inputs name, then ask mbx what it resolved.
 *
 * Inputs win over the environment, but a setting without an input is left as
 * an earlier step exported it. mbx's own report decides whether that adds up
 * to a usable remote, since only mbx knows every place a URL can come from.
 */
async function configureRemote(mbx: string): Promise<RemoteStatus> {
  const variables = remoteExports({
    url: aliasedInput('remote-url', core.getInput('remote-url'), 'server-url', core.getInput('server-url')),
    namespace: core.getInput('namespace'),
    token: core.getInput('token'),
    tokenFile: core.getInput('token-file'),
    oidcAudience: core.getInput('oidc-audience'),
    mode: aliasedInput(
      'remote-mode',
      core.getInput('remote-mode'),
      'server-mode',
      core.getInput('server-mode')
    )
  })
  if (variables.MBX_REMOTE_TOKEN) core.setSecret(variables.MBX_REMOTE_TOKEN)
  for (const [name, value] of Object.entries(variables)) core.exportVariable(name, value)

  let report = ''
  let spawnError = ''
  try {
    await exec.exec(mbx, ['doctor', '--json'], {
      ignoreReturnCode: true,
      silent: true,
      listeners: {stdout: data => (report += data.toString())}
    })
  } catch (error) {
    spawnError = String(error)
  }
  // A doctor that never started says why in the one warning the unknown state
  // already produces, rather than in a debug line nobody sees.
  const status = spawnError
    ? {state: 'unknown' as const, detail: `mbx doctor could not run: ${spawnError}`}
    : remoteStatus(report)
  switch (status.state) {
    case 'missing':
      throw new Error(
        'The remote backend found no remote cache to use. Set the remote-url and namespace ' +
          'inputs, export MBX_REMOTE_URL and MBX_REMOTE_NAMESPACE in an earlier step, or ' +
          "configure [remote] in mbx's user config file."
      )
    case 'invalid':
      throw new Error(`mbx rejects the remote cache configuration: ${status.detail}`)
    case 'unreachable':
      core.warning(`mbx could not reach the remote cache: ${status.detail}`)
      break
    case 'unknown':
      core.warning(`Could not confirm the remote cache configuration: ${status.detail}`)
      break
    case 'ready':
      core.info(`Remote cache: ${status.detail}`)
      break
  }
  return status
}

async function main(): Promise<void> {
  const backend = parseBackend(core.getInput('backend'))
  const githubCacheMode = parseGithubCacheMode(core.getInput('github-cache-mode'))
  const isolateObjectsCache = core.getBooleanInput('isolate-objects-cache')
  if (isolateObjectsCache && (backend !== 'github' || githubCacheMode !== 'objects')) {
    throw new Error('isolate-objects-cache requires backend github and github-cache-mode objects')
  }
  const targetCache = backend === 'github' && githubCacheMode === 'target'
  if (backend === 'github') requireGithubCacheRuntime()
  const gcAuto = githubObjectGcDefault(backend, githubCacheMode)
  if (gcAuto !== undefined) {
    core.exportVariable('MBX_GC_AUTO', gcAuto)
    core.info('Disabled automatic mbx cache GC for this GitHub-hosted object-cache job')
  }
  const githubToken = githubTokenValue(core.getInput('github-token'))
  if (githubToken) core.setSecret(githubToken)
  let installed = targetCache ? undefined : await setupMbx(core.getInput('version'), githubToken)
  const cacheLinks =
    targetCache
      ? '0'
      : cacheLinksValue(core.getInput('cache-links'), process.platform)
  if (cacheLinks !== undefined) core.exportVariable('MBX_CACHE_LINKS', cacheLinks)

  if (backend === 'local') {
    if (!installed) throw new Error('mbx setup did not complete')
    core.info(`Set up mbx ${installed.version}`)
    core.setOutput('mbx-version', installed.version)
    core.saveState(POST_STATE, backend)
    core.saveState(MBX_STATE, installed.bin)
    core.exportVariable('MBX_REMOTE_URL', '')
    const cacheDir = await capture(installed.bin, ['cache', 'dir'])
    await mkdir(cacheDir, {recursive: true})
    await leaveCallingCard('Everything is being kept on the premises.', [
      {label: 'mbx', value: installed.version},
      {label: 'Backend', value: 'local filesystem'},
      {label: 'Cache', value: cacheDir}
    ])
    return
  }

  if (backend === 'remote') {
    if (!installed) throw new Error('mbx setup did not complete')
    core.info(`Set up mbx ${installed.version}`)
    core.setOutput('mbx-version', installed.version)
    core.saveState(POST_STATE, backend)
    core.saveState(MBX_STATE, installed.bin)
    const remote = await configureRemote(installed.bin)
    await leaveCallingCard('I have made the necessary arrangements.', [
      {label: 'mbx', value: installed.version},
      {label: 'Backend', value: 'remote cache'},
      {label: 'Remote', value: remote.detail},
      ...(remote.policy ? [{label: 'Mode', value: remote.policy}] : [])
    ])
    return
  }

  let cacheArchive = ''
  let bundleForm: BundleForm = 'tar'
  let isolatedObjectsPaths: ObjectsCachePaths | undefined
  if (githubCacheMode === 'objects') {
    if (!installed) throw new Error('mbx setup did not complete')
    bundleForm = supportsDirectoryBundle(installed.version) ? 'directory' : 'tar'
    if (isolateObjectsCache) {
      const runnerTemp = process.env.RUNNER_TEMP
      if (!runnerTemp) throw new Error('RUNNER_TEMP is required by isolate-objects-cache')
      isolatedObjectsPaths = await createObjectsCachePaths(runnerTemp, bundleForm)
      core.exportVariable('MBX_CACHE_DIR', isolatedObjectsPaths.store)
      const isolatedActionStore = path.join(isolatedObjectsPaths.store, 'actions')
      const resolvedActionStore = await capture(installed.bin, ['cache', 'dir'])
      assertIsolatedObjectsActionStore(isolatedObjectsPaths, resolvedActionStore)
      await mkdir(isolatedActionStore, {recursive: true})
      cacheArchive = isolatedObjectsPaths.bundle
    } else {
      const cacheDir = await capture(installed.bin, ['cache', 'dir'])
      await mkdir(cacheDir, {recursive: true})
      cacheArchive = path.join(
        cacheDir,
        bundleForm === 'directory' ? CACHE_BUNDLE_NAME : CACHE_ARCHIVE_NAME
      )
    }
  }
  const exportGroup =
    githubCacheMode === 'objects'
      ? `github-actions-${context.runId}-${context.runAttempt}-${randomUUID()}`
      : ''
  if (exportGroup) core.exportVariable('MBX_CACHE_EXPORT_GROUP', exportGroup)
  if (githubCacheMode === 'target') {
    core.exportVariable('MBX_REMOTE_URL', '')
    core.exportVariable('MBX_TARGET_VIEWS', '0')
  }
  const generation = githubCacheGeneration(
    core.getInput('cache-generation'),
    githubCacheMode,
    bundleForm
  )
  const requestedToolchain = core.getInput('toolchain')
  const toolchain = toolchainSegment(await rustcIdentity(requestedToolchain))
  if (toolchain === 'norust') {
    // A named toolchain that will not answer is a louder failure than no Rust
    // at all: the caller has said which compiler the build uses, and keying the
    // store as if it had none puts it back in the shared bucket the input was
    // reached for to escape.
    if (requestedToolchain) {
      core.warning(
        `Could not ask the ${requestedToolchain} toolchain for its identity; the generated ` +
          'cache key carries none. Install that toolchain before this action.'
      )
    } else {
      core.info(
        'No rustc found on PATH; the generated cache key carries no toolchain identity. ' +
          'Install the Rust toolchain before this action so a toolchain update starts a fresh cache.'
      )
    }
  }
  const defaultBranch = (context.payload.repository as {default_branch?: string} | undefined)
    ?.default_branch
  const {save, reason: saveReason} = savePolicy(
    {
      eventName: context.eventName,
      ref: context.ref,
      defaultBranch,
      refProtected: process.env.GITHUB_REF_PROTECTED === 'true',
      cacheMode: process.env.ACTIONS_CACHE_MODE,
      sameRepository: isSameRepositoryPullRequest(
        context.payload.pull_request as PullRequestRepositories | undefined
      )
    },
    {
      workflowDispatch: core.getBooleanInput('save-on-workflow-dispatch'),
      pullRequest: core.getBooleanInput('save-on-pull-request'),
      protectedBranch: core.getBooleanInput('save-on-protected-branch')
    }
  )
  const baseSha = context.payload.pull_request?.base.sha ?? context.sha
  const sha = cacheRevision(context.eventName, baseSha, save, context.runId, context.runAttempt)
  const primaryKey = primaryCacheKey(
    core.getInput('cache-key'),
    core.getInput('cache-key-suffix'),
    generatedKey(process.platform, process.arch, generation, toolchain, sha)
  )
  const explicitRestoreKeys = core.getMultilineInput('restore-keys').filter(Boolean)
  const generatedRestoreKeys: string[] = []
  if (explicitRestoreKeys.length === 0) {
    if (save && context.eventName === 'pull_request') {
      generatedRestoreKeys.push(
        pullRequestRestoreKey(process.platform, process.arch, generation, toolchain, baseSha)
      )
    }
    generatedRestoreKeys.push(
      generatedRestoreKey(process.platform, process.arch, generation, toolchain)
    )
  }
  const restoreKeys = effectiveRestoreKeys(explicitRestoreKeys, generatedRestoreKeys)
  const cargoHome = process.env.CARGO_HOME || path.join(homedir(), '.cargo')
  const targetToolDirectory = path.join(
    process.env.RUNNER_TEMP || path.join(homedir(), '.cache'),
    TARGET_TOOL_DIRECTORY
  )
  const targetDirectory = cargoTargetDirectory(core.getInput('working-directory'))
  const cargoWorkspace = path.dirname(targetDirectory)
  if (targetCache && !(await isDirectory(cargoWorkspace))) {
    throw new Error(`working-directory ${JSON.stringify(cargoWorkspace)} is not a directory`)
  }
  const targetPaths = [
    targetDirectory,
    path.join(cargoHome, 'registry'),
    path.join(cargoHome, 'git'),
    targetToolDirectory
  ]
  const cachePaths = githubCacheMode === 'target' ? targetPaths : [cacheArchive]
  if (isolatedObjectsPaths) {
    core.saveState(CACHE_ISOLATION_ROOT_STATE, isolatedObjectsPaths.root)
    await reportObjectsResourcePhase(
      isolatedObjectsPaths,
      'before-restore-import',
      targetDirectory,
      message => core.info(message)
    )
  }
  const restoredKey = await cache.restoreCache(cachePaths, primaryKey, restoreKeys)
  if (targetCache) {
    installed = await stageTargetCacheMbx(
      await setupMbx(core.getInput('version'), githubToken, targetToolDirectory),
      targetToolDirectory
    )
  }
  if (!installed) throw new Error('mbx setup did not complete')
  core.info(`Set up mbx ${installed.version}`)
  core.setOutput('mbx-version', installed.version)
  core.saveState(POST_STATE, backend)
  core.saveState(MBX_STATE, installed.bin)
  if (restoredKey && githubCacheMode === 'objects') {
    if (isolatedObjectsPaths) {
      await importObjectsBundle(isolatedObjectsPaths, async bundlePath => {
        await reportObjectsResourcePhase(
          isolatedObjectsPaths!,
          'before-cache-import',
          targetDirectory,
          message => core.info(message)
        )
        await exec.exec(installed!.bin, ['cache', 'import', bundlePath])
      })
    } else {
      await exec.exec(installed.bin, ['cache', 'import', cacheArchive])
    }
  } else if (isolatedObjectsPaths) {
    await assertObjectsBundleAbsent(isolatedObjectsPaths)
  } else if (restoredKey && githubCacheMode === 'target') {
    const hydrated = await hydrateMbxShimBinaries(targetDirectory, installed.bin)
    if (hydrated > 0) core.info(`Restored ${hydrated} mbx build-script shim binaries`)
  }
  const hit = restoredKey === primaryKey
  core.setOutput('cache-hit', hit ? 'true' : 'false')
  core.setOutput('cache-primary-key', primaryKey)
  core.info(restoredKey ? `Restored mbx cache from ${restoredKey}` : 'No mbx cache found')

  core.saveState(CACHE_ARCHIVE_STATE, cacheArchive)
  core.saveState(CACHE_BUNDLE_FORM_STATE, bundleForm)
  if (isolatedObjectsPaths) core.saveState(CACHE_ISOLATION_ROOT_STATE, isolatedObjectsPaths.root)
  core.saveState(CACHE_EXPORT_GROUP_STATE, exportGroup)
  core.saveState(CACHE_PATHS_STATE, JSON.stringify(cachePaths))
  core.saveState(CARGO_WORKSPACE_STATE, cargoWorkspace)
  core.saveState(CACHE_KEY_STATE, primaryKey)
  core.saveState(CACHE_HIT_STATE, hit ? 'true' : 'false')
  core.saveState(
    POST_STATE,
    save ? 'github-save' : 'github-restore-only'
  )
  core.setOutput('cache-save-eligible', save ? 'true' : 'false')
  core.setOutput('cache-save-reason', saveReason)
  core.info(
    save
      ? `Will save the mbx cache after a successful job (${saveReason})`
      : `Restore only (${saveReason})`
  )
  const cacheResult = hit ? 'exact hit' : restoredKey ? 'warm start' : 'miss'
  const note = hit
    ? 'Just as I left it.'
    : restoredKey
      ? 'Not precisely what I ordered, but quite serviceable.'
      : 'The cupboard was bare. How stimulating.'
  await leaveCallingCard(note, [
    {label: 'mbx', value: installed.version},
    {label: 'Backend', value: 'GitHub Actions cache'},
    {
      label: 'Payload',
      value:
        githubCacheMode === 'target'
          ? 'Cargo target tree'
          : bundleForm === 'directory'
            ? 'mbx objects (directory)'
            : 'mbx objects (tar)'
    },
    {label: 'Cache', value: cacheResult},
    {
      label: 'Policy',
      value: save
        ? `save after a successful job (${saveReason})`
        : `restore only (${saveReason})`
    }
  ])
}

async function post(): Promise<void> {
  const postState = core.getState(POST_STATE)
  const isolationRoot = core.getState(CACHE_ISOLATION_ROOT_STATE)
  if (isolationRoot && (postState === 'github-save' || postState === 'github-restore-only')) {
    const savedForm = core.getState(CACHE_BUNDLE_FORM_STATE)
    if (savedForm !== 'directory' && savedForm !== 'tar') {
      throw new Error('isolated objects cache has an invalid bundle form in action state')
    }
    const form: BundleForm = savedForm
    const runnerTemp = process.env.RUNNER_TEMP
    if (!runnerTemp) throw new Error('RUNNER_TEMP is required by isolate-objects-cache post step')
    const isolatedPaths = await pathsFromObjectsCacheRoot(runnerTemp, isolationRoot, form)
    const result = await withIsolatedObjectsPostCleanup(
      isolatedPaths,
      async () => {
        if (core.getState(CACHE_ARCHIVE_STATE) !== isolatedPaths.bundle) {
          throw new Error('isolated objects cache bundle path does not match action state')
        }
        const primaryKey = core.getState(CACHE_KEY_STATE)
        if (!primaryKey) throw new Error('isolated objects cache is missing its primary key')
        const cargoWorkspace = core.getState(CARGO_WORKSPACE_STATE) || process.cwd()
        const targetDirectory = path.join(cargoWorkspace, 'target')
        return saveIsolatedObjectsBundle({
          paths: isolatedPaths,
          primaryKey,
          saveEligible: postState === 'github-save',
          exactHit: core.getState(CACHE_HIT_STATE) === 'true',
          cargoTarget: targetDirectory,
          exportBundle: async bundlePath => {
            const mbx = core.getState(MBX_STATE)
            if (!mbx) throw new Error('isolated objects cache is missing its mbx executable')
            const resolvedActionStore = await capture(mbx, ['cache', 'dir'])
            assertIsolatedObjectsActionStore(isolatedPaths, resolvedActionStore)
            const group = core.getState(CACHE_EXPORT_GROUP_STATE)
            if (!group) throw new Error('isolated objects cache is missing its export group')
            const exportArgs =
              form === 'directory'
                ? ['cache', 'export', '--group', group, '--format', 'directory', bundlePath]
                : ['cache', 'export', '--group', group, bundlePath]
            let output = ''
            const exitCode = await exec.exec(mbx, exportArgs, {
              ignoreReturnCode: true,
              listeners: {
                stdout: data => (output += data.toString()),
                stderr: data => (output += data.toString())
              }
            })
            return {exitCode, output}
          },
          isEmptyExport,
          saveCache: (paths, key) => cache.saveCache(paths, key),
          emit: message => core.info(message),
          warn: message => core.warning(message)
        })
      },
      message => core.warning(message)
    )
    if (result === 'exact-hit') {
      core.info(`Exact cache ${core.getState(CACHE_KEY_STATE)} already exists; not saving it again`)
    }
    return
  }
  if (postState !== 'github-save') return
  const primaryKey = core.getState(CACHE_KEY_STATE)
  if (core.getState(CACHE_HIT_STATE) === 'true') {
    core.info(`Exact cache ${primaryKey} already exists; not saving it again`)
    return
  }
  const mbx = core.getState(MBX_STATE)
  const archive = core.getState(CACHE_ARCHIVE_STATE)
  const group = core.getState(CACHE_EXPORT_GROUP_STATE)
  const paths = JSON.parse(core.getState(CACHE_PATHS_STATE)) as string[]
  if (!group) {
    const cargoWorkspace = core.getState(CARGO_WORKSPACE_STATE) || process.cwd()
    const targetDirectory = path.join(cargoWorkspace, 'target')
    if (!(await hasReusableCargoTarget(targetDirectory))) {
      core.info('No reusable Cargo target state was produced; not saving a registry-only cache')
      return
    }
    const metadata = await capture('cargo', ['metadata', '--format-version', '1'], cargoWorkspace)
    const cargoHome = process.env.CARGO_HOME || path.join(homedir(), '.cargo')
    await pruneCargoTargetCache(targetDirectory, cargoHome, metadata)
    const dehydrated = await dehydrateMbxShimBinaries(targetDirectory)
    if (dehydrated > 0) core.info(`Omitted ${dehydrated} mbx build-script shim binaries`)
    const cacheId = await cache.saveCache(paths, primaryKey)
    core.info(`Saved mbx target cache ${primaryKey} (ID ${cacheId})`)
    return
  }
  const bundleForm = (core.getState(CACHE_BUNDLE_FORM_STATE) || 'tar') as BundleForm
  const exportArgs =
    bundleForm === 'directory'
      ? ['cache', 'export', '--group', group, '--format', 'directory', archive]
      : ['cache', 'export', '--group', group, archive]
  let output = ''
  const exportExitCode = await exec.exec(mbx, exportArgs, {
    ignoreReturnCode: true,
    listeners: {
      stdout: data => (output += data.toString()),
      stderr: data => (output += data.toString())
    }
  })
  if (exportExitCode !== 0) {
    if (isEmptyExport(output)) {
      core.info('No completed mbx build was recorded; not saving an empty cache')
      return
    }
    throw new Error(`mbx cache export exited with code ${exportExitCode}`)
  }
  const cacheId = await cache.saveCache([archive], primaryKey)
  core.info(`Saved mbx cache ${primaryKey} (ID ${cacheId})`)
}

const isPost = Boolean(core.getState(POST_STATE))
;(isPost ? post() : main()).catch(error => core.setFailed(error instanceof Error ? error : String(error)))
