// demonstrates: per-package "other" snippets shown in
//   docs/src/stdlib/other.md
// docs: docs/src/stdlib/other.md
// platforms: macos, linux
// Windows excluded: this complete-file example uses try/catch, which the
// default Windows RS4GC pipeline intentionally refuses until WinEH funclet
// statepoints are supported (#7354).
// run: false

// Each ANCHOR block below is the exact code that the other-modules docs page
// renders inline (via {{#include ... :NAME}}). The whole file is compiled
// and linked by the doc-tests harness — `run: false` because nodemailer
// connects to an SMTP server and child_process spawns + sleeps a real
// process, neither hermetic in CI. Compile + link is the contract here.
//
// Only packages with wired NativeModSig dispatch (nodemailer,
// child_process) are anchored. sharp / cheerio /
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
const _keep = [nodemailerExample, lruCacheExample, childProcessExample]
console.log(`other-snippets: ${_keep.length}`)
