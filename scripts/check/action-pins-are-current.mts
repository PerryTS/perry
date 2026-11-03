#!/usr/bin/env node
/** Check upstream action pins and Perry's workflow ownership policy. */
import { realpathSync } from 'node:fs'
import process from 'node:process'
import { pathToFileURL } from 'node:url'
import { checkActionPins } from '../update-actions.mts'

export function main(): Promise<number> {
  return checkActionPins()
}

// Compare resolved paths: argv[1] keeps a symlinked prefix that import.meta.url
// has resolved away.
const isMain =
  process.argv[1] !== undefined &&
  pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url
if (isMain) main().then((code) => { process.exitCode = code }).catch((error: unknown) => {
  console.error('[check:actions] ' + (error instanceof Error ? error.message : String(error)))
  process.exitCode = 1
})
