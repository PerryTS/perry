// #9591: fs.watch / fsPromises.watch are driven by OS change notifications
// (inotify / FSEvents / ReadDirectoryChangesW), not by a 25 ms timer that
// re-walked the whole tree. This fixture pins the observable contract that
// every backend shares with node: a change inside the watched target surfaces
// as an event naming that entry, relative to the root, on the callback
// watcher, on a single-file watcher, and on the promise-based async iterator.
//
// Only what every platform agrees on is printed. Event TYPES differ between
// inotify and FSEvents even under node (a create is one 'rename' on Linux,
// possibly 'rename' + 'change' on macOS), so the first event naming the file
// each phase creates is what gets reported — never the type or the count.
import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "perry-9591-"));
const TIMEOUT_MS = 5000;

// libuv arms an FSEvents stream asynchronously (on its CF run-loop thread), so
// under node on macOS a write issued in the same tick as fs.watch() can land
// before the stream exists and never be reported. Give every new watcher a
// moment to arm before the change it is meant to see; inotify needs none of
// this, and the cost is a few hundred milliseconds of wall time.
const settle = () => new Promise((resolve) => setTimeout(resolve, 250));

function firstEventNamed(watcher: any, expected: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error("timeout waiting for " + expected)),
      TIMEOUT_MS,
    );
    watcher.on("change", (_eventType: string, filename: any) => {
      const name = String(filename).replace(/\\/g, "/");
      if (name === expected) {
        clearTimeout(timer);
        resolve(name);
      }
    });
  });
}

async function main() {
  // 1. Non-recursive directory watch: a new direct child.
  {
    const watcher = fs.watch(root);
    const seen = firstEventNamed(watcher, "a.txt");
    await settle();
    fs.writeFileSync(path.join(root, "a.txt"), "one");
    console.log("watch:", await seen);
    watcher.close();
  }

  // 2. Recursive watch: a file two levels down reports its relative path.
  {
    fs.mkdirSync(path.join(root, "sub"));
    const watcher = fs.watch(root, { recursive: true });
    const seen = firstEventNamed(watcher, "sub/b.txt");
    await settle();
    fs.writeFileSync(path.join(root, "sub", "b.txt"), "two");
    console.log("recursive:", await seen);
    watcher.close();
  }

  // 3. A single-file watch reports the file's own name.
  {
    const file = path.join(root, "a.txt");
    const watcher = fs.watch(file);
    const seen = firstEventNamed(watcher, "a.txt");
    await settle();
    fs.writeFileSync(file, "one-more");
    console.log("file:", await seen);
    watcher.close();
  }

  // 4. fsPromises.watch: the async iterator yields { eventType, filename }.
  //    The OS watch starts with the first next(), so the write is scheduled
  //    for after iteration begins (and after the stream has armed, as above).
  {
    const ac = new AbortController();
    const guard = setTimeout(() => ac.abort(), TIMEOUT_MS);
    setTimeout(() => fs.writeFileSync(path.join(root, "c.txt"), "three"), 250);
    try {
      for await (const event of fsp.watch(root, { signal: ac.signal })) {
        const name = String(event.filename).replace(/\\/g, "/");
        if (name === "c.txt") {
          console.log("promises:", name);
          break;
        }
      }
    } catch (err: any) {
      console.log("promises: aborted", err && err.name);
    } finally {
      clearTimeout(guard);
    }
  }

  // 5. Close is idempotent and 'close' fires once.
  {
    const watcher = fs.watch(root);
    let closes = 0;
    watcher.on("close", () => {
      closes++;
    });
    watcher.close();
    watcher.close();
    await new Promise((r) => setTimeout(r, 20));
    console.log("close events:", closes);
  }

  fs.rmSync(root, { recursive: true, force: true });
  console.log("done");
}

main().catch((err) => {
  console.log("failed:", err && err.message);
});
