import * as core from '@actions/core'
import {run} from './merge-artifacts.js'

run().catch(error => {
  core.setFailed((error as Error).message)
})
