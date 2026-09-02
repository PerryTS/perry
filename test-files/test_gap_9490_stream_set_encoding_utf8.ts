// #9490: a stream with `setEncoding("utf8")` must decode like Node's
// `string_decoder` — U+FFFD for every invalid sequence, and carry-over state
// for a multi-byte sequence split across a chunk boundary.
//
// Perry's `stdin_chunk_jsvalue` handed the raw bytes straight to
// `js_string_from_bytes`, which does no validation at all: it memcpy's the
// bytes into the string payload and only *counts* UTF-16 units with a
// WTF-8-shaped walk. Feeding bytes 0..255 through `setEncoding("utf8")`:
//
//            code units   U+FFFD   high bytes
//   node        256         128    replaced per WHATWG
//   perry       158           0    raw U+0080..U+00FF passed through
//
// Two failures at once — invalid sequences were not replaced, and 98 code
// units simply vanished (the WTF-8 length walk consumed continuation bytes
// into the preceding lead byte). `Buffer.toString("utf8")` was already
// correct (`buf_bytes_to_utf8_string` runs `String::from_utf8_lossy`); only
// the stream path was wrong.
//
// The damage in claude-code: it writes what it read into the session
// transcript, so perry's JSONL lines carried raw 0x80..0xFF bytes — neither
// valid UTF-8 nor parseable JSON — and `--resume` could not read the session
// back.
//
// The second half of the contract is carry-over. A per-chunk decode is not
// enough: an emoji split across two reads must NOT become two replacement
// characters. Node holds the incomplete tail until the next chunk and
// flushes it as U+FFFD at `end`. Node emits that flush as its own final
// `'data'` event and emits NO event for a chunk that decodes to "" — both
// pinned below by the per-event breakdown.
//
// The parity runner gives a fixture no stdin, so this test re-spawns itself
// with a pipe on the child's stdin and drives each shape in a child role.
import { spawn } from "node:child_process";
import { existsSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const ROLE_ENV = "PERRY_9490_ROLE";
const READY_ENV = "PERRY_9490_READY_FILE";
const role = process.env[ROLE_ENV] ?? "";

const ALL_256 = Array.from({ length: 256 }, (_, i) => i);
// Lone continuation, 2-byte overlong, 3-byte overlong, UTF-16 surrogate
// encoded as UTF-8, and the two bytes that can never appear in UTF-8 — each
// separated by an ASCII comma so a position shift is visible in the output.
const INVALID = [0x80, 0x2c, 0xc0, 0x80, 0x2c, 0xe0, 0x80, 0xaf, 0x2c, 0xed, 0xa0, 0x80, 0x2c, 0xfe, 0x2c, 0xff];
// 'A' then a truncated 3-byte sequence, so the stream ends mid-character.
const TRUNC_END = [0x41, 0xe2, 0x82];
// Three 4-byte emoji, written in four spaced pieces that split the first
// after 1 byte, the second after 2, and the third after 3.
const EMOJI = [0xf0, 0x9f, 0x98, 0x80];
const SPLIT_WRITES = [
  EMOJI.slice(0, 1),
  [...EMOJI.slice(1), ...EMOJI.slice(0, 2)],
  [...EMOJI.slice(2), ...EMOJI.slice(0, 3)],
  EMOJI.slice(3),
];
const SPLIT_DELAY_MS = 150;

function bytesFor(name: string): number[] {
  if (name === "invalid") return INVALID;
  if (name === "truncend") return TRUNC_END;
  return ALL_256;
}

// Compact, diffable rendering of a string's UTF-16 code units.
function units(s: string): string {
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i).toString(16));
  return out.join(" ");
}
function fffdCount(s: string): number {
  let n = 0;
  for (let i = 0; i < s.length; i++) if (s.charCodeAt(i) === 0xfffd) n += 1;
  return n;
}

// ---------------------------------------------------------------------------
// Child roles
// ---------------------------------------------------------------------------
if (role !== "") {
  const stdin = process.stdin;
  stdin.setEncoding("utf8");
  const events: string[] = [];
  let acc = "";

  const finish = () => {
    console.log(role + " length:", acc.length);
    console.log(role + " fffd:", fffdCount(acc));
    if (role === "all256" || role === "pull") {
      // Exact code units, and the ASCII prefix must survive untouched.
      console.log(role + " ascii ok:", units(acc.slice(0, 128)) === units(ALL_256.slice(0, 128).map((b) => String.fromCharCode(b)).join("")));
      console.log(role + " tail units:", units(acc.slice(128)));
    } else {
      console.log(role + " units:", units(acc));
    }
    if (role === "truncend" || role === "splits") {
      // Per-event breakdown: proves the incomplete tail is HELD (no empty
      // event, no premature replacement) and flushed as its own last event.
      console.log(role + " events:", JSON.stringify(events));
    }
  };

  if (role === "pull") {
    // Paused / pull mode: `on("readable")` plus `read()`, the other half of
    // the readable-side contract.
    stdin.on("readable", () => {
      let chunk = stdin.read();
      while (chunk !== null) {
        const text = String(chunk);
        if (text.length > 0) {
          events.push(units(text));
          acc += text;
        }
        chunk = stdin.read();
      }
    });
  } else {
    stdin.on("data", (chunk: string) => {
      events.push(units(chunk));
      acc += chunk;
    });
  }
  stdin.on("end", finish);

  const readyPath = process.env[READY_ENV] ?? "";
  if (readyPath !== "") writeFileSync(readyPath, "1");
} else {
  // -------------------------------------------------------------------------
  // Parent: drive each role in a child with a real pipe on its stdin.
  // -------------------------------------------------------------------------
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];

  const runRole = (name: string) =>
    new Promise<void>((resolve) => {
      const readyPath = join(tmpdir(), "perry-9490-ready-" + name + "-" + process.pid);
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
      if (name === "splits") {
        // Each piece must reach the child as its OWN read, or the split
        // never happens and the arm proves nothing. Wait for the child to
        // have its listener up, then space the writes.
        let i = 0;
        const step = () => {
          if (i >= SPLIT_WRITES.length) {
            child.stdin!.end();
            return;
          }
          child.stdin!.write(Buffer.from(SPLIT_WRITES[i]));
          i += 1;
          setTimeout(step, SPLIT_DELAY_MS);
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
        child.stdin!.end(Buffer.from(bytesFor(name)));
      }
    });

  (async () => {
    await runRole("all256");
    await runRole("invalid");
    await runRole("truncend");
    await runRole("splits");
    await runRole("pull");
    console.log("done");
  })();
}
