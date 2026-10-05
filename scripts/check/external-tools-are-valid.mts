#!/usr/bin/env node
/** Check external tool version and integrity pins. */
import process from 'node:process'
import { pathToFileURL } from 'node:url'
import { checkExternalToolPins } from '../soak/external-tools.mts'

export function main(): number {
  return checkExternalToolPins()
}

const isMain = process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url
if (isMain) process.exitCode = main()
