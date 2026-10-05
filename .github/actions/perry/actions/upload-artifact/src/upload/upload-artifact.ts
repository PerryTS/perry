import * as core from '@actions/core'
import artifact, {
  UploadArtifactOptions,
  ArtifactNotFoundError
} from '@actions/artifact'
import {findFilesToUpload} from '../shared/search.js'
import {getInputs} from './input-helper.js'
import {NoFileOptions} from './constants.js'
import {uploadArtifact} from '../shared/upload-artifact.js'

async function deleteArtifactIfExists(artifactName: string): Promise<void> {
  try {
    await artifact.deleteArtifact(artifactName)
  } catch (error) {
    if (error instanceof ArtifactNotFoundError) {
      core.debug(`Skipping deletion of '${artifactName}', it does not exist`)
      return
    }

    // Best effort, we don't want to fail the action if this fails
    core.debug(`Unable to delete artifact: ${(error as Error).message}`)
  }
}

export async function run(): Promise<void> {
  const inputs = getInputs()
  const searchResult = await findFilesToUpload(
    inputs.searchPath,
    inputs.includeHiddenFiles
  )
  if (searchResult.filesToUpload.length === 0) {
    // No files were found, different use cases warrant different types of behavior if nothing is found
    switch (inputs.ifNoFilesFound) {
      case NoFileOptions.warn: {
        core.warning(
          `No files were found with the provided path: ${inputs.searchPath}. No artifacts will be uploaded.`
        )
        break
      }
      case NoFileOptions.error: {
        core.setFailed(
          `No files were found with the provided path: ${inputs.searchPath}. No artifacts will be uploaded.`
        )
        break
      }
      case NoFileOptions.ignore: {
        core.info(
          `No files were found with the provided path: ${inputs.searchPath}. No artifacts will be uploaded.`
        )
        break
      }
    }
  } else {
    const s = searchResult.filesToUpload.length === 1 ? '' : 's'
    core.info(
      `With the provided path, there will be ${searchResult.filesToUpload.length} file${s} uploaded`
    )
    core.debug(`Root artifact directory is ${searchResult.rootDirectory}`)

    // Validate that only a single file is uploaded when archive is false
    if (!inputs.archive && searchResult.filesToUpload.length > 1) {
      core.setFailed(
        `When 'archive' is set to false, only a single file can be uploaded. Found ${searchResult.filesToUpload.length} files to upload.`
      )
      return
    }

    if (inputs.overwrite) {
      await deleteArtifactIfExists(inputs.artifactName)
    }

    const options: UploadArtifactOptions = {}
    if (inputs.retentionDays) {
      options.retentionDays = inputs.retentionDays
    }

    if (typeof inputs.compressionLevel !== 'undefined') {
      options.compressionLevel = inputs.compressionLevel
    }

    if (!inputs.archive) {
      options.skipArchive = true
    }

    await uploadArtifact(
      inputs.artifactName,
      searchResult.filesToUpload,
      searchResult.rootDirectory,
      options
    )
  }
}
