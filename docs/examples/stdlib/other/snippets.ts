// demonstrates: per-package "other" snippets shown in
//   docs/src/stdlib/other.md
// docs: docs/src/stdlib/other.md
// platforms: macos, linux, windows
// run: false

// Each ANCHOR block below is the exact code that the other-modules docs page
// renders inline (via {{#include ... :NAME}}). The whole file is compiled
// and linked by the doc-tests harness — `run: false` because nodemailer
// connects to an SMTP server and child_process spawns + sleeps a real
// process, neither hermetic in CI. Compile + link is the contract here.
//
// Only packages with wired NativeModSig dispatch (nodemailer, commander,
// decimal.js, lru-cache, child_process) are anchored. sharp / cheerio /
// zlib / cron / worker_threads have runtime declarations but no dispatch
// path from user-visible imports yet, so the markdown page keeps those
// snippets as `,no-test` with a clear status note above each fence.

// ANCHOR: nodemailer
import nodemailer from "nodemailer"

async function nodemailerExample(): Promise<void> {
    const transporter = nodemailer.createTransport({
        host: "smtp.example.com",
        port: 587,
        auth: { user: "user", pass: "pass" },
    })

    await transporter.sendMail({
        from: "sender@example.com",
        to: "recipient@example.com",
        subject: "Hello from Perry",
        text: "This email was sent from a compiled TypeScript binary!",
    })
}
// ANCHOR_END: nodemailer

// ANCHOR: commander
import { Command } from "commander"

function commanderExample(): void {
    const program = new Command()
    program.name("my-cli").version("1.0.0").description("My CLI tool")

    program
        .command("serve")
        .option("-p, --port <number>", "Port number")
        .option("--verbose", "Verbose output")
        .action((options: any) => {
            console.log(`Starting server on port ${options.port}`)
        })

    program.parse(process.argv)
}
// ANCHOR_END: commander

// ANCHOR: decimal
import Decimal from "decimal.js"

function decimalExample(): void {
    const a = new Decimal("0.1")
    const b = new Decimal("0.2")
    const sum = a.plus(b) // Exactly 0.3 (no floating point errors)

    console.log(sum.toFixed(2))      // "0.30"
    console.log(sum.toNumber())      // 0.3
    console.log(a.times(b).toFixed(2)) // "0.02"
    console.log(a.div(b).toFixed(1))   // "0.5"
    console.log(a.pow(10).toString())  // 1e-10
    console.log(a.sqrt().toFixed(3))   // "0.316"
}
// ANCHOR_END: decimal

// ANCHOR: lru-cache
import { LRUCache } from "lru-cache"

function lruCacheExample(): void {
    const cache = new LRUCache({ max: 100 }) // max 100 entries

    cache.set("key", "value")
    console.log(cache.get("key"))   // "value"
    console.log(cache.has("key"))   // true
    cache.delete("key")
    cache.clear()
}
// ANCHOR_END: lru-cache

// ANCHOR: child-process
// `spawnBackground` / `getProcessStatus` / `killProcess` are Perry EXTENSIONS —
// Node's `child_process` has no such named exports, so importing them by name
// is rejected (correctly) with U006. Reach them through the module namespace.
import * as child_process from "child_process"

function childProcessExample(): void {
    // Spawn a background process
    const { pid, handleId } = child_process.spawnBackground("sleep", ["10"], "/tmp/log.txt")

    // Check if it's still running
    const status = child_process.getProcessStatus(handleId)
    console.log(status.alive) // true
    console.log(`pid=${pid}`)

    // Kill it
    child_process.killProcess(handleId)
}
// ANCHOR_END: child-process

// Reference everything so unused-import elimination doesn't strip it.
const _keep = [nodemailerExample, commanderExample, decimalExample, lruCacheExample, childProcessExample]
console.log(`other-snippets: ${_keep.length}`)
