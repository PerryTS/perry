#!/usr/bin/env node
/** Check all configured dependency release-age soak surfaces. */
import { realpathSync } from 'node:fs'
import process from 'node:process'
import { pathToFileURL } from 'node:url'
import { checkSoakSurfaces } from '../soak/soak.mts'

export function main(): number {
  return checkSoakSurfaces()
}

// Compare resolved paths: argv[1] keeps a symlinked prefix that import.meta.url
// has resolved away.
const isMain =
  process.argv[1] !== undefined &&
  pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url
if (isMain) process.exitCode = main()
