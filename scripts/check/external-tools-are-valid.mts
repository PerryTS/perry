#!/usr/bin/env node
/** Check external tool version and integrity pins. */
import { realpathSync } from 'node:fs'
import process from 'node:process'
import { pathToFileURL } from 'node:url'
import { checkExternalToolPins } from '../soak/external-tools.mts'

export function main(): number {
  return checkExternalToolPins()
}

// Compare resolved paths: argv[1] keeps a symlinked prefix that import.meta.url
// has resolved away.
const isMain =
  process.argv[1] !== undefined &&
  pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url
if (isMain) process.exitCode = main()
