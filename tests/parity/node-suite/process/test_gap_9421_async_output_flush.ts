// #9421 — the async queue-and-flush write path, pinned.
//
// NOT a gap test: every shape below already passes on unfixed `main`, and that
// is the point. #9421 reports that a claude-code session transcript comes out
// 1 line where Node writes 5, and attributes it to the session writer's async
// `insertQueueOperation` → `flush` path losing records ("work enqueued
// asynchronously and flushed before exit is lost; sync writes land"). This
// fixture is that attribution's test: it drives the shapes the report names,
// including a faithful transliteration of claude-code's own `SessionWriter`
// (`scheduleDrain` → `setTimeout(FLUSH_INTERVAL_MS = 100)` → `await
// drainWriteQueue()` → `await appendFile`, alongside the one `appendFileSync`
// record the report says is the only survivor). Perry matches Node in all of
// them, so the async-flush attribution is wrong and the divergence is upstream
// of the writer.
//
// The 1-vs-5 signature does have an exact cause, and this fixture pins it too:
// `writer-exit-early` exits before the 100 ms drain timer fires and lands
// exactly ONE record — the synchronous one — under BOTH engines. So the
// symptom identifies a run that ended too early, not a flush that failed.
import { spawn } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const ROLE_ENV = "PERRY_9421_ROLE";
const role = process.env[ROLE_ENV] ?? "";
const BIG_LINE = "y".repeat(1023) + "\n";
const BIG_LINES = 200; // 204800 bytes — comfortably past one pipe buffer

function transcript(): string {
  return path.join(os.tmpdir(), "perry_9421_" + process.pid + ".jsonl");
}

// claude-code's session writer, transliterated from the bundle.
class SessionWriter {
  queues = new Map<string, { entry: unknown; resolve: () => void }[]>();
  flushTimer: ReturnType<typeof setTimeout> | null = null;
  activeDrain: Promise<void> | null = null;
  FLUSH_INTERVAL_MS = 100;

  enqueueWrite(file: string, entry: unknown): Promise<void> {
    return new Promise<void>((resolve) => {
      let q = this.queues.get(file);
      if (!q) {
        q = [];
        this.queues.set(file, q);
      }
      q.push({ entry, resolve });
      this.scheduleDrain();
    });
  }

  scheduleDrain(): void {
    if (this.flushTimer) return;
    this.flushTimer = setTimeout(async () => {
      this.flushTimer = null;
      this.activeDrain = this.drainWriteQueue();
      await this.activeDrain;
      this.activeDrain = null;
      if (this.queues.size > 0) this.scheduleDrain();
    }, this.FLUSH_INTERVAL_MS);
  }

  async drainWriteQueue(): Promise<void> {
    for (const [file, q] of this.queues) {
      if (q.length === 0) continue;
      const batch = q.splice(0);
      let chunk = "";
      for (const item of batch) chunk += JSON.stringify(item.entry) + "\n";
      await fs.promises.appendFile(file, chunk, { mode: 0o600 });
      for (const item of batch) item.resolve();
    }
    for (const [file, q] of this.queues) if (q.length === 0) this.queues.delete(file);
  }
}

function driveWriter(exitAfterMs: number | null): void {
  const file = transcript();
  const writer = new SessionWriter();
  for (let i = 0; i < 4; i++) void writer.enqueueWrite(file, { type: "queued", i: i });
  // The one record the report says survives: a direct synchronous append.
  fs.appendFileSync(file, JSON.stringify({ type: "last-prompt" }) + "\n");
  const report = () => {
    let lines: string[] = [];
    try {
      lines = fs.readFileSync(file, "utf8").split("\n").filter((l) => l.length > 0);
    } catch {
      /* nothing written */
    }
    try {
      fs.unlinkSync(file);
    } catch {
      /* already gone */
    }
    console.log("records:", lines.length);
    for (const line of lines) console.log("  " + line);
  };
  if (exitAfterMs === null) {
    // Natural drain: read the file back one turn after the drain must have run.
    setTimeout(report, 400);
  } else {
    setTimeout(() => {
      report();
      process.exit(0);
    }, exitAfterMs);
  }
}

if (role === "async-callbacks") {
  // Multi-line output produced from async callbacks.
  for (let i = 0; i < 5; i++) Promise.resolve().then(() => console.log("promise " + i));
  process.nextTick(() => console.log("tick"));
  setTimeout(() => console.log("timer"), 1);
} else if (role === "write-loop") {
  for (let i = 0; i < 5; i++) process.stdout.write("write " + i + "\n");
} else if (role === "interleaved") {
  console.log("out 1");
  console.error("err 1");
  console.log("out 2");
  console.error("err 2");
  console.log("out 3");
} else if (role === "write-then-exit") {
  for (let i = 0; i < 5; i++) process.stdout.write("exit-write " + i + "\n");
  console.log("exit-write done");
  process.exit(0);
} else if (role === "big") {
  for (let i = 0; i < BIG_LINES; i++) process.stdout.write(BIG_LINE);
} else if (role === "writer-natural") {
  driveWriter(null);
} else if (role === "writer-exit-late") {
  driveWriter(300);
} else if (role === "writer-exit-early") {
  // Exits before the 100 ms drain timer: ONE record, the synchronous one.
  driveWriter(40);
} else {
  const childArgs = [...process.execArgv, ...process.argv.slice(1)];
  const runRole = (name: string) =>
    new Promise<void>((resolve) => {
      // `interleaved` inherits both streams so the two writes keep their real
      // relative order in the fixture's own merged output; separate pipes
      // would only prove per-stream ordering.
      const stdio: ("ignore" | "pipe" | "inherit")[] =
        name === "interleaved" ? ["ignore", "inherit", "inherit"] : ["ignore", "pipe", "pipe"];
      const child = spawn(process.execPath, childArgs, {
        env: { ...process.env, [ROLE_ENV]: name },
        stdio: stdio,
      });
      let out = "";
      let err = "";
      child.stdout?.on("data", (chunk: Buffer | string) => {
        out += String(chunk);
      });
      child.stderr?.on("data", (chunk: Buffer | string) => {
        err += String(chunk);
      });
      child.on("close", (code) => {
        console.log("== " + name + " exit: " + code);
        // `big` is compared by size so the fixture's own output stays small.
        if (name === "big") {
          console.log("stdout bytes: " + out.length);
        } else {
          for (const line of out.split("\n")) if (line.length > 0) console.log("out| " + line);
        }
        for (const line of err.split("\n")) if (line.length > 0) console.log("err| " + line);
        resolve();
      });
    });

  (async () => {
    await runRole("async-callbacks");
    await runRole("write-loop");
    await runRole("interleaved");
    await runRole("write-then-exit");
    await runRole("big");
    await runRole("writer-natural");
    await runRole("writer-exit-late");
    await runRole("writer-exit-early");
    console.log("done");
  })();
}
