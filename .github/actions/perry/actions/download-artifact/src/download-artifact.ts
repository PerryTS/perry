import * as os from 'os'
import * as path from 'path'
import * as core from '@actions/core'
import artifactClient from '@actions/artifact'
import type {Artifact, FindOptions} from '@actions/artifact'
import {Minimatch} from 'minimatch'
import {Inputs, Outputs, DigestMismatchBehavior} from './constants.js'

const PARALLEL_DOWNLOADS = 5

export const chunk = <T>(arr: T[], n: number): T[][] =>
  arr.reduce((acc, cur, i) => {
    const index = Math.floor(i / n)
    acc[index] = [...(acc[index] || []), cur]
    return acc
  }, [] as T[][])

export async function run(): Promise<void> {
  const inputs = {
    name: core.getInput(Inputs.Name, {required: false}),
    path: core.getInput(Inputs.Path, {required: false}),
    token: core.getInput(Inputs.GitHubToken, {required: false}),
    repository: core.getInput(Inputs.Repository, {required: false}),
    runID: parseInt(core.getInput(Inputs.RunID, {required: false})),
    pattern: core.getInput(Inputs.Pattern, {required: false}),
    mergeMultiple: core.getBooleanInput(Inputs.MergeMultiple, {
      required: false
    }),
    artifactIds: core.getInput(Inputs.ArtifactIds, {required: false}),
    skipDecompress: core.getBooleanInput(Inputs.SkipDecompress, {
      required: false
    }),
    digestMismatch: (core.getInput(Inputs.DigestMismatch, {required: false}) ||
      DigestMismatchBehavior.Error) as DigestMismatchBehavior
  }

  // Validate digest-mismatch input
  const validBehaviors = Object.values(DigestMismatchBehavior)
  if (!validBehaviors.includes(inputs.digestMismatch)) {
    throw new Error(
      `Invalid value for 'digest-mismatch': '${inputs.digestMismatch}'. Valid options are: ${validBehaviors.join(', ')}`
    )
  }

  if (!inputs.path) {
    inputs.path = process.env['GITHUB_WORKSPACE'] || process.cwd()
  }

  if (inputs.path.startsWith(`~`)) {
    inputs.path = inputs.path.replace('~', os.homedir())
  }

  // Check for mutually exclusive inputs
  if (inputs.name && inputs.artifactIds) {
    throw new Error(
      `Inputs 'name' and 'artifact-ids' cannot be used together. Please specify only one.`
    )
  }

  const isSingleArtifactDownload = !!inputs.name
  const isDownloadByIds = !!inputs.artifactIds
  const resolvedPath = path.resolve(inputs.path)
  core.debug(`Resolved path is ${resolvedPath}`)

  const options: FindOptions = {}
  if (inputs.token) {
    const [repositoryOwner, repositoryName] = inputs.repository.split('/')
    if (!repositoryOwner || !repositoryName) {
      throw new Error(
        `Invalid repository: '${inputs.repository}'. Must be in format owner/repo`
      )
    }

    options.findBy = {
      token: inputs.token,
      workflowRunId: inputs.runID,
      repositoryName,
      repositoryOwner
    }
  }

  let artifacts: Artifact[] = []
  let artifactIds: number[] = []

  if (isSingleArtifactDownload) {
    core.info(`Downloading single artifact`)

    const {artifact: targetArtifact} = await artifactClient.getArtifact(
      inputs.name,
      options
    )

    if (!targetArtifact) {
      throw new Error(`Artifact '${inputs.name}' not found`)
    }

    core.debug(
      `Found named artifact '${inputs.name}' (ID: ${targetArtifact.id}, Size: ${targetArtifact.size})`
    )

    artifacts = [targetArtifact]
  } else if (isDownloadByIds) {
    core.info(`Downloading artifacts by ID`)

    const artifactIdList = inputs.artifactIds
      .split(',')
      .map(id => id.trim())
      .filter(id => id !== '')

    if (artifactIdList.length === 0) {
      throw new Error(`No valid artifact IDs provided in 'artifact-ids' input`)
    }

    core.debug(`Parsed artifact IDs: ${JSON.stringify(artifactIdList)}`)

    // Parse the artifact IDs
    artifactIds = artifactIdList.map(id => {
      const numericId = parseInt(id, 10)
      if (isNaN(numericId)) {
        throw new Error(`Invalid artifact ID: '${id}'. Must be a number.`)
      }
      return numericId
    })

    // We need to fetch all artifacts to get metadata for the specified IDs
    const listArtifactResponse = await artifactClient.listArtifacts({
      latest: true,
      ...options
    })

    artifacts = listArtifactResponse.artifacts.filter(artifact =>
      artifactIds.includes(artifact.id)
    )

    if (artifacts.length === 0) {
      throw new Error(`None of the provided artifact IDs were found`)
    }

    if (artifacts.length < artifactIds.length) {
      const foundIds = artifacts.map(a => a.id)
      const missingIds = artifactIds.filter(id => !foundIds.includes(id))
      core.warning(
        `Could not find the following artifact IDs: ${missingIds.join(', ')}`
      )
    }

    core.debug(`Found ${artifacts.length} artifacts by ID`)
  } else {
    const listArtifactResponse = await artifactClient.listArtifacts({
      latest: true,
      ...options
    })
    artifacts = listArtifactResponse.artifacts

    core.debug(`Found ${artifacts.length} artifacts in run`)

    if (inputs.pattern) {
      core.info(`Filtering artifacts by pattern '${inputs.pattern}'`)
      const matcher = new Minimatch(inputs.pattern)
      artifacts = artifacts.filter(artifact => matcher.match(artifact.name))
      core.debug(
        `Filtered from ${listArtifactResponse.artifacts.length} to ${artifacts.length} artifacts`
      )
    } else {
      core.info(
        'No input name, artifact-ids or pattern filtered specified, downloading all artifacts'
      )
      if (!inputs.mergeMultiple) {
        core.info(
          'An extra directory with the artifact name will be created for each download'
        )
      }
    }
  }

  if (artifacts.length) {
    core.info(`Preparing to download the following artifacts:`)
    artifacts.forEach(artifact => {
      core.info(
        `- ${artifact.name} (ID: ${artifact.id}, Size: ${artifact.size}, Expected Digest: ${artifact.digest})`
      )
    })
  }

  const downloadPromises = artifacts.map(artifact => ({
    name: artifact.name,
    promise: artifactClient.downloadArtifact(artifact.id, {
      ...options,
      path:
        isSingleArtifactDownload ||
        inputs.mergeMultiple ||
        artifacts.length === 1
          ? resolvedPath
          : path.join(resolvedPath, artifact.name),
      expectedHash: artifact.digest,
      skipDecompress: inputs.skipDecompress
    })
  }))

  const chunkedPromises = chunk(downloadPromises, PARALLEL_DOWNLOADS)
  const digestMismatches: string[] = []

  for (const chunk of chunkedPromises) {
    const chunkPromises = chunk.map(item => item.promise)
    const results = await Promise.all(chunkPromises)

    for (let i = 0; i < results.length; i++) {
      const outcome = results[i]
      const artifactName = chunk[i].name

      if (outcome.digestMismatch) {
        digestMismatches.push(artifactName)
        const message = `Artifact '${artifactName}' digest validation failed. Please verify the integrity of the artifact.`

        switch (inputs.digestMismatch) {
          case DigestMismatchBehavior.Ignore:
            // Do nothing
            break
          case DigestMismatchBehavior.Info:
            core.info(message)
            break
          case DigestMismatchBehavior.Warn:
            core.warning(message)
            break
          case DigestMismatchBehavior.Error:
            // Collect all errors and fail at the end
            break
        }
      }
    }
  }

  // If there were digest mismatches and behavior is 'error', fail the action
  if (
    digestMismatches.length > 0 &&
    inputs.digestMismatch === DigestMismatchBehavior.Error
  ) {
    throw new Error(
      `Digest validation failed for artifact(s): ${digestMismatches.join(', ')}. ` +
        `Use 'digest-mismatch: warn' to continue on mismatch.`
    )
  }

  core.info(`Total of ${artifacts.length} artifact(s) downloaded`)
  core.setOutput(Outputs.DownloadPath, resolvedPath)
  core.info('Download artifact has finished successfully')
}

run().catch(err =>
  core.setFailed(`Unable to download artifact(s): ${err.message}`)
)
