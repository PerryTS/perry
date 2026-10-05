import * as core from '@actions/core'
import {run} from './upload-artifact.js'

run().catch(error => {
  core.setFailed((error as Error).message)
})
