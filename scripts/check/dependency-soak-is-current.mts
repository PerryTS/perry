#!/usr/bin/env node
/** Check all configured dependency release-age soak surfaces. */
import process from 'node:process'
import { pathToFileURL } from 'node:url'
import { checkSoakSurfaces } from '../soak/soak.mts'

export function main(): number {
  return checkSoakSurfaces()
}

const isMain = process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url
if (isMain) process.exitCode = main()
