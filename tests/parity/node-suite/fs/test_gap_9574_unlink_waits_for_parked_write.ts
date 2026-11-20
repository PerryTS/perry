// #9574: Perry parks async writeFile/appendFile work on a later event-loop
// turn, but its async unlink entry points normally perform their filesystem
// operation synchronously. An unlink issued while a same-path write was still
// parked therefore saw ENOENT; the write then ran and recreated the file.
//
// Claude Code exposed the race during graceful shutdown: registration of
// ~/.claude/sessions/<pid>.json was still parked when its cleanup callback
// unlinked that path. Node had completed the earlier write and removed the
// file; Perry rejected the unlink and left a nondeterministic stale record.
//
// The intervening stat is deliberate. It models the unrelated awaited fs work
// in startup and gives Node's earlier thread-pool write time to complete. Perry
// resolves stat synchronously, so this failed deterministically before #9574.
import * as fs from "node:fs";
import {
  appendFile as appendFileAsync,
  stat as statAsync,
  unlink as unlinkAsync,
  writeFile as writeFileAsync,
} from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";

const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "perry9574-"));

function codeOf(error: unknown): string {
  return (error as { code?: string }).code ?? "error";
}

async function promiseCase(): Promise<void> {
  const file = path.join(tmpDir, "promise.json");
  const write = writeFileAsync(file, "session\n");
  await statAsync(tmpDir);

  let unlinkResult = "ok";
  try {
    await unlinkAsync(file);
  } catch (error) {
    unlinkResult = codeOf(error);
  }
  await write;
  console.log(
    "promise unlink=" +
      unlinkResult +
      " final=" +
      (fs.existsSync(file) ? "exists" : "missing"),
  );
}

async function callbackCase(): Promise<void> {
  const file = path.join(tmpDir, "callback.json");
  const write = new Promise<void>((resolve, reject) => {
    fs.writeFile(file, "session\n", (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
  await statAsync(tmpDir);

  const unlinkResult = await new Promise<string>((resolve) => {
    fs.unlink(file, (error) => resolve(error ? codeOf(error) : "ok"));
  });
  await write;
  console.log(
    "callback unlink=" +
      unlinkResult +
      " final=" +
      (fs.existsSync(file) ? "exists" : "missing"),
  );
}

async function multipleWritesCase(): Promise<void> {
  const file = path.join(tmpDir, "multiple.json");
  const writes = [
    appendFileAsync(file, "one\n"),
    appendFileAsync(file, "two\n"),
  ];
  await statAsync(tmpDir);

  let unlinkResult = "ok";
  try {
    await unlinkAsync(file);
  } catch (error) {
    unlinkResult = codeOf(error);
  }
  await Promise.all(writes);
  console.log(
    "multiple unlink=" +
      unlinkResult +
      " final=" +
      (fs.existsSync(file) ? "exists" : "missing"),
  );
}

(async () => {
  await promiseCase();
  await callbackCase();
  await multipleWritesCase();
  fs.rmSync(tmpDir, { recursive: true, force: true });
  console.log("done");
})();
