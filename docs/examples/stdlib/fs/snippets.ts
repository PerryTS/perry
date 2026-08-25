// demonstrates: per-API fs/path snippets shown in docs/src/stdlib/fs.md
// docs: docs/src/stdlib/fs.md
// platforms: macos, linux
// Windows excluded: this complete-file example uses try/catch, which the
// default Windows RS4GC pipeline intentionally refuses until WinEH funclet
// statepoints are supported (#7354).

// Each ANCHOR block below is the exact code that the fs docs page renders
// inline (via {{#include ... :NAME}}). The whole file is compiled and run by
// the doc-tests harness, so every snippet is a tested artifact — if any
// snippet drifts from the real fs / path API, CI fails.
//
// We use os.tmpdir() throughout so the file is portable across macOS/Linux
// (where /tmp exists) and Windows (where it doesn't).

// `readFileBuffer` is a Perry EXTENSION, not part of Node's `fs` named-export
// surface — Node itself has no such export, so `import { readFileBuffer }`
// is rejected (correctly) with U006. Reach it through the module namespace.
import * as fs from "fs"
import {
    readFileSync,
    writeFileSync,
    existsSync,
    statSync,
    mkdirSync,
    readdirSync,
    rmdirSync,
    unlinkSync,
} from "fs"
import { join, dirname, basename, resolve } from "path"
import { tmpdir } from "os"

// All filesystem snippets share a per-run scratch directory so they don't
// stomp on each other and so the file is hermetic.
const scratch = join(tmpdir(), "perry_docs_fs_snippets")
if (!existsSync(scratch)) mkdirSync(scratch)

// Pre-seed the files the read snippets expect.
writeFileSync(join(scratch, "config.json"), `{"name":"perry"}`)
// "image.png" stand-in: 8 bytes is enough to exercise readFileBuffer.
writeFileSync(join(scratch, "image.png"), "PERRYBIN")

// ANCHOR: read-text
const configPath = join(scratch, "config.json")
const content = readFileSync(configPath, "utf-8")
console.log(content)
// ANCHOR_END: read-text

// ANCHOR: read-binary
const imagePath = join(scratch, "image.png")
const buffer = fs.readFileBuffer(imagePath)
console.log(`Read ${buffer.length} bytes`)
// ANCHOR_END: read-binary

// ANCHOR: write-text
const outputPath = join(scratch, "output.txt")
const dataPath = join(scratch, "data.json")
writeFileSync(outputPath, "Hello, World!")
writeFileSync(dataPath, JSON.stringify({ key: "value" }, null, 2))
// ANCHOR_END: write-text

// ANCHOR: stat
if (existsSync(configPath)) {
    const stat = statSync(configPath)
    console.log(`Size: ${stat.size}`)
}
// ANCHOR_END: stat

// ANCHOR: dirs
// Create directory
const outDir = join(scratch, "output")
if (!existsSync(outDir)) mkdirSync(outDir)

// Read directory contents
const files = readdirSync(scratch)
for (const file of files) {
    console.log(file)
}

// Remove an empty directory
rmdirSync(outDir)
// ANCHOR_END: dirs

// ANCHOR: path-utils
const dir = dirname(configPath)
const cfgPath = join(dir, "config.json")
const name = basename(cfgPath)        // "config.json"
const abs = resolve("relative/path")  // Absolute path
console.log(`${name} ${abs.length > 0}`)
// ANCHOR_END: path-utils

// Cleanup so reruns start fresh.
unlinkSync(join(scratch, "config.json"))
unlinkSync(join(scratch, "image.png"))
unlinkSync(join(scratch, "output.txt"))
unlinkSync(join(scratch, "data.json"))
rmdirSync(scratch)
