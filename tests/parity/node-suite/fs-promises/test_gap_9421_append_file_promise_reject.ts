// #9421: `fs/promises.appendFile` must REJECT when the append fails.
//
// Perry routed the promise form through the status-returning sync helper
// (`js_fs_append_file_sync_options`, `0` = failure) and then dropped the
// status with `let _ = ...`, so the returned Promise ALWAYS resolved. The
// same helper backed `fs.appendFileSync` (silently did nothing instead of
// throwing) and `fs.appendFile(path, data, cb)` (called back with no error).
//
// The cost was invisible until a caller used the rejection as CONTROL FLOW.
// The claude-code session writer does exactly that:
//
//     async appendToFile(p, chunk) {
//       try { await appendFile(p, chunk, { mode: 0o600 }) }
//       catch { await mkdir(dirname(p), { recursive: true, mode: 0o700 })
//               await appendFile(p, chunk, { mode: 0o600 }) }
//     }
//
// The transcript directory `~/.claude/projects/<slug>/` is created lazily --
// by this very catch. Under Perry the first append "succeeded", the recovery
// arm never ran, no directory was ever created, and every queued transcript
// record was discarded with no error anywhere. `claude --bare -p hi` wrote 1
// record where Node wrote 5; the survivor was the one record written through
// `openSync` + `appendFileSync(fd, ...)`, which does throw.
//
// Pre-fix Perry prints `resolved` / `no-throw` / `no-err` on the first three
// probes and `writer-file: MISSING`. Node -- and fixed Perry -- print
// `ENOENT` three times and the writer's two records.
//
// The `writer-mode` line pins the second half of the same defect: the `mode`
// the writer passes was dropped on the create path, so the transcript landed
// `0644` where Node makes it `0600`.

import { appendFile, mkdir, readFile } from "node:fs/promises";
import {
  appendFileSync,
  mkdtempSync,
  openSync,
  closeSync,
  rmSync,
  statSync,
  appendFile as appendFileCb,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";

const root = mkdtempSync(join(tmpdir(), "perry9421-"));
const missing = join(root, "nodir", "out.jsonl");

function code(e: unknown): string {
  const c = (e as { code?: string } | null)?.code;
  return typeof c === "string" ? c : "NO-CODE";
}

async function main(): Promise<void> {
  // 1. promise form
  try {
    await appendFile(missing, "x\n", { mode: 0o600 });
    console.log("promises.appendFile: resolved");
  } catch (e) {
    console.log("promises.appendFile: " + code(e));
  }

  // 2. sync form
  try {
    appendFileSync(missing, "x\n", { mode: 0o600 });
    console.log("appendFileSync: no-throw");
  } catch (e) {
    console.log("appendFileSync: " + code(e));
  }

  // 3. callback form
  await new Promise<void>((resolve) => {
    appendFileCb(missing, "x\n", (err) => {
      console.log("appendFile(cb): " + (err ? code(err) : "no-err"));
      resolve();
    });
  });

  // 4. The session-writer shape: the catch is the only thing that ever
  //    creates the directory.
  const target = join(root, "projects", "session.jsonl");
  async function appendToFile(p: string, chunk: string): Promise<void> {
    try {
      await appendFile(p, chunk, { mode: 0o600 });
    } catch {
      await mkdir(dirname(p), { recursive: true, mode: 0o700 });
      await appendFile(p, chunk, { mode: 0o600 });
    }
  }
  await appendToFile(target, '{"type":"queue-operation"}\n');
  await appendToFile(target, '{"type":"user"}\n');
  try {
    const text = await readFile(target, "utf8");
    const lines = text.split("\n").filter((l) => l.length > 0);
    console.log("writer-records: " + lines.length);
    for (const l of lines) console.log("writer-line: " + l);
    // The writer asks for 0o600; a transcript is private. Perry used to drop
    // the mode and create it 0666 & ~umask, i.e. world-readable.
    console.log("writer-mode: " + (statSync(target).mode & 0o777).toString(8));
  } catch (e) {
    console.log("writer-file: MISSING (" + code(e) + ")");
  }

  // 5. The happy paths still work: fresh file, grow it, then append via an fd.
  const good = join(root, "good.txt");
  await appendFile(good, "one\n");
  await appendFile(good, "two\n");
  const fd = openSync(good, "a");
  appendFileSync(fd, "three\n");
  closeSync(fd);
  console.log("good: " + JSON.stringify(await readFile(good, "utf8")));

  rmSync(root, { recursive: true, force: true });
}

main();
