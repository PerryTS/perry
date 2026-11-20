// #9489: `process.stdin` must chunk by READ-BUFFER boundaries, not by lines.
//
// Perry's fd-0 reader read stdin ONE BYTE PER `read(2)` and, in cooked
// flowing / pull mode, cut a `'data'` chunk at every `\n`. 1 MB of
// `"line\n"` therefore arrived as 200,000 `'data'` events in 2.10 s where
// Node delivers 16 in 0.04 s — a 52x slowdown driven by 1,048,576 read
// syscalls plus 200,000 JS callback dispatches. Input with no newline in it
// arrived as a single chunk, which is what pinned the cause on newlines
// rather than on size.
//
// The user-visible damage was in claude-code: under the event flood its
// stdin consumer gave up partway, so a piped prompt was truncated to a
// RANDOM prefix (612k / 455k / 394k / 374k chars of 1,000,000 across four
// runs). Feeding the same megabyte as one chunk, or as 1,000 x 1,000-char
// lines, produced the full prompt every time.
//
// Node's contract: a chunk is whatever one read of the underlying handle
// returned. Line splitting belongs to the consumer (readline does its own),
// never to the shared `'data'` path.
//
// The parity runner gives a fixture no stdin, so this test re-spawns itself
// with a pipe on the child's stdin and drives each shape in a child role.
//
// Event counts cannot be compared byte-for-byte across engines — Node's pipe
// reads are 64 KiB, Perry's are 64 KiB but land on a different tick boundary
// — so the flood arms assert an ORDER OF MAGNITUDE (`< 100`, against Node's
// 16 and the unfixed engine's 200,000) plus exact byte counts and exact
// content. The `trickle` arm is the opposite guard: three writes spaced in
// time must stay THREE events, so a "fix" that simply glues the whole stream
// into one chunk fails here.
import { spawn } from "node:child_process";
import { existsSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const ROLE_ENV = "PERRY_9489_STDIN_ROLE";
const READY_ENV = "PERRY_9489_READY_FILE";
const role = process.env[ROLE_ENV] ?? "";

const TOTAL = 1_000_000;
const LINES_PAYLOAD = "line\n".repeat(TOTAL / 5); // 200,000 lines
const FLAT_PAYLOAD = "x".repeat(TOTAL); // no newline at all
const K1000_PAYLOAD = ("y".repeat(999) + "\n").repeat(TOTAL / 1000); // 1,000 lines
const TRICKLE_PARTS = ["one\n", "two\n", "three\n"];
const TRICKLE_DELAY_MS = 150;

function payloadFor(name: string): string {
  if (name === "lines") return LINES_PAYLOAD;
  if (name === "flat") return FLAT_PAYLOAD;
  if (name === "k1000") return K1000_PAYLOAD;
  return TRICKLE_PARTS.join("");
}

// ---------------------------------------------------------------------------
// Child roles
// ---------------------------------------------------------------------------
if (role !== "") {
  const expected = payloadFor(role);
  let events = 0;
  let bytes = 0;
  let acc = "";
  // The literal `process.stdin.on(...)` spelling is the one codegen lowers to
  // perry-stdlib's readline extern — the path that did the line splitting.
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk: string) => {
    events += 1;
    bytes += chunk.length;
    acc += chunk;
  });
  // Readiness handshake: the `trickle` arm measures how the reader SPLITS
  // three spaced writes, which is only meaningful once the reader exists.
  // Child start-up costs a few hundred ms under `--experimental-strip-types`,
  // so without this the parent's three writes all land in the pipe buffer
  // before the child reads anything and arrive as ONE chunk under Node too.
  const readyPath = process.env[READY_ENV] ?? "";
  if (readyPath !== "") writeFileSync(readyPath, "1");
  process.stdin.on("end", () => {
    console.log(role + " bytes:", bytes);
    console.log(role + " content ok:", acc === expected);
    if (role === "trickle") {
      // Three spaced writes must not be glued into one chunk.
      console.log(role + " events >= 3:", events >= 3);
      console.log(role + " events <= 10:", events <= 10);
    } else {
      console.log(role + " events >= 1:", events >= 1);
      console.log(role + " events < 100:", events < 100);
    }
  });
} else {
  // -------------------------------------------------------------------------
  // Parent: drive each role in a child with a real pipe on its stdin.
  // -------------------------------------------------------------------------
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const runRole = (name: string) =>
    new Promise<void>((resolve) => {
      const readyPath = join(tmpdir(), "perry-9489-ready-" + name + "-" + process.pid);
      try {
        rmSync(readyPath, { force: true });
      } catch {}
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name, [READY_ENV]: readyPath },
        stdio: ["pipe", "inherit", "inherit"],
      });
      child.on("exit", (code) => {
        try {
          rmSync(readyPath, { force: true });
        } catch {}
        console.log(name + " exit:", code);
        resolve();
      });
      if (name === "trickle") {
        // Spaced writes: each must reach the child as its own read.
        let i = 0;
        const step = () => {
          if (i >= TRICKLE_PARTS.length) {
            child.stdin!.end();
            return;
          }
          child.stdin!.write(TRICKLE_PARTS[i]);
          i += 1;
          setTimeout(step, TRICKLE_DELAY_MS);
        };
        const waitReady = () => {
          if (existsSync(readyPath)) {
            step();
            return;
          }
          setTimeout(waitReady, 20);
        };
        waitReady();
      } else {
        child.stdin!.end(payloadFor(name));
      }
    });

  (async () => {
    await runRole("lines");
    await runRole("flat");
    await runRole("k1000");
    await runRole("trickle");
    console.log("done");
  })();
}
