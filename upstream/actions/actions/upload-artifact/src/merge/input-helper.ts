import * as core from '@actions/core'
import {Inputs} from './constants.js'
import {MergeInputs} from './merge-inputs.js'

/**
 * Helper to get all the inputs for the action
 */
export function getInputs(): MergeInputs {
  const name = core.getInput(Inputs.Name, {required: true})
  const pattern = core.getInput(Inputs.Pattern, {required: true})
  const separateDirectories = core.getBooleanInput(Inputs.SeparateDirectories)
  const deleteMerged = core.getBooleanInput(Inputs.DeleteMerged)
  const includeHiddenFiles = core.getBooleanInput(Inputs.IncludeHiddenFiles)

  const inputs = {
    name,
    pattern,
    separateDirectories,
    deleteMerged,
    retentionDays: 0,
    compressionLevel: 6,
    includeHiddenFiles
  } as MergeInputs

  const retentionDaysStr = core.getInput(Inputs.RetentionDays)
  if (retentionDaysStr) {
    inputs.retentionDays = parseInt(retentionDaysStr)
    if (isNaN(inputs.retentionDays)) {
      core.setFailed('Invalid retention-days')
    }
  }

  const compressionLevelStr = core.getInput(Inputs.CompressionLevel)
  if (compressionLevelStr) {
    inputs.compressionLevel = parseInt(compressionLevelStr)
    if (isNaN(inputs.compressionLevel)) {
      core.setFailed('Invalid compression-level')
    }

    if (inputs.compressionLevel < 0 || inputs.compressionLevel > 9) {
      core.setFailed('Invalid compression-level. Valid values are 0-9')
    }
  }

  return inputs
}
