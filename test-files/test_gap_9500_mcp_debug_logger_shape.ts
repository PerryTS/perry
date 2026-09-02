// #9500: claude-code's MCP debug logger wrote NOTHING under perry — not even
// the `~/.cache/claude-cli-nodejs/<key>/mcp-logs-<server>/` tree — although
// the connect-failure path that feeds it demonstrably ran. This fixture is the
// bundle's exact write shape, de-minified:
//
//   * every fs call goes through a wrapper compiled from a `using` declaration
//     (esbuild's downlevel): the error is stashed by `var O=A,w=1` in the CATCH
//     block and re-thrown from FINALLY by the dispose helper;
//   * records go into a buffered writer flushed by a 1 s timer, a size cap, or
//     `dispose()`; the logger registers `dispose` in a cleanup set that the
//     graceful-shutdown path awaits (raced against a 2 s timer) before
//     `process.exit`;
//   * the flush's write function is `try { appendFileSync } catch { mkdirSync;
//     appendFileSync }` — the ONLY code that ever creates the log directory
//     tree, so it relies on the first append THROWING ENOENT.
//
// Under perry the append silently succeeded-without-writing (#9421, fixed for
// this surface by #9491), the recovery arm never ran, and the tree was never
// created. This pins the whole shape end to end: the throw, the `using`
// re-throw, the recursive mkdir recovery, the timer/dispose flush, and the
// exit sequencing.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// ── esbuild's `using` downlevel helpers, verbatim shape ──────────────────────
const SYM_DISPOSE = Symbol.dispose || Symbol.for("Symbol.dispose");
const SYM_ASYNC_DISPOSE = Symbol.asyncDispose || Symbol.for("Symbol.asyncDispose");
const rz = (q: any[], K: any, _: any) => {
  if (K != null) {
    if (typeof K !== "object" && typeof K !== "function") throw TypeError('Object expected to be assigned to "using" declaration');
    var z;
    if (_) z = K[SYM_ASYNC_DISPOSE];
    if (z === void 0) z = K[SYM_DISPOSE];
    if (typeof z !== "function") throw TypeError("Object not disposable");
    q.push([_, z, K]);
  } else if (_) q.push([_]);
  return K;
};
const oz = (q: any[], K: any, _: any) => {
  var z = typeof (globalThis as any).SuppressedError === "function"
      ? (globalThis as any).SuppressedError
      : function (O: any, w: any, $: any, j?: any) { return (j = Error($)), (j.name = "SuppressedError"), (j.error = O), (j.suppressed = w), j; },
    Y = (O: any) => (K = _ ? new z(O, K, "An error was suppressed during disposal") : ((_ = !0), O)),
    A: any = (O?: any) => {
      while ((O = q.pop()))
        try {
          var w = O[1] && O[1].call(O[2]);
          if (O[0]) return Promise.resolve(w).then(A, ($: any) => (Y($), A()));
        } catch ($) { Y($); }
      if (_) throw K;
    };
  return A();
};
// Tracing is off in the shipped CLI: the span tag yields nothing disposable.
function Jw(_s: TemplateStringsArray, ..._v: any[]): any { return undefined; }

// ── the fs wrapper (`V8()`), the two methods the logger uses ────────────────
const V8 = {
  appendFileSync(q: string, K: string) { let Y: any[] = []; try { const _ = rz(Y, Jw`fs.appendFileSync(${q})`, 0); fs.appendFileSync(q, K); } catch (A) { var O = A, w = 1; } finally { oz(Y, O, w); } },
  mkdirSync(q: string) { let Y: any[] = []; try { const _ = rz(Y, Jw`fs.mkdirSync(${q})`, 0); try { fs.mkdirSync(q, { recursive: true }); } catch ($: any) { if ($?.code !== "EEXIST") throw $; } } catch (A) { var O = A, w = 1; } finally { oz(Y, O, w); } },
};

// ── the buffered writer (`bD6`) ─────────────────────────────────────────────
function bufferedWriter({ writeFn, flushIntervalMs = 1000, maxBufferSize = 100 }: { writeFn: (s: string) => void; flushIntervalMs?: number; maxBufferSize?: number }) {
  let buf: string[] = [], timer: any = null, pending: string[] | null = null;
  const clear = () => { if (timer) clearTimeout(timer), (timer = null); };
  const flush = () => { if (pending) writeFn(pending.join("")), (pending = null); if (buf.length === 0) return; writeFn(buf.join("")), (buf = []), clear(); };
  const arm = () => { if (!timer) timer = setTimeout(flush, flushIntervalMs); };
  const flushSoon = () => { if (pending) { pending.push(...buf), (buf = []), clear(); return; } const M = buf; buf = [], clear(), (pending = M), setImmediate(() => { const P = pending; if (((pending = null), P)) writeFn(P.join("")); }); };
  return { write(M: string) { buf.push(M), arm(), buf.length >= maxBufferSize && flushSoon(); }, flush, dispose() { flush(); } };
}

// ── the cleanup registry (`eq` / `_w8`) ─────────────────────────────────────
const cleanups = new Set<() => Promise<unknown>>();
function onCleanup(fn: () => Promise<unknown>) { cleanups.add(fn); return () => cleanups.delete(fn); }
async function runCleanups() { await Promise.all(Array.from(cleanups).map((q) => q())); }

// ── the per-file logger cache (`BJ7`) and the MCP sinks ─────────────────────
let recoveries = 0;
const loggers = new Map<string, ReturnType<typeof bufferedWriter>>();
function loggerFor(file: string) {
  let K = loggers.get(file);
  if (!K) {
    const dir = path.dirname(file);
    const w = bufferedWriter({
      writeFn: (z) => { try { V8.appendFileSync(file, z); } catch { recoveries++; V8.mkdirSync(dir); V8.appendFileSync(file, z); } },
      flushIntervalMs: 1000,
      maxBufferSize: 50,
    });
    K = { write: (o: unknown) => w.write(JSON.stringify(o) + "\n"), flush: w.flush, dispose: w.dispose };
    loggers.set(file, K);
    onCleanup(async () => K?.dispose());
  }
  return K;
}
const home = fs.mkdtempSync(path.join(os.tmpdir(), "gap9500-"));
const cacheRoot = path.join(home, ".cache", "claude-cli-nodejs", "-cwd-key"); // nothing under `home` exists yet
const mcpLogPath = (server: string) => path.join(cacheRoot, `mcp-logs-${server}`, "session.jsonl");
function logMCPDebug(server: string, msg: string) { loggerFor(mcpLogPath(server)).write({ debug: msg, timestamp: "T" }); }
function logMCPError(server: string, err: unknown) { loggerFor(mcpLogPath(server)).write({ error: err instanceof Error ? err.message : String(err), timestamp: "T" }); }

// ── the connect-failure path, then graceful shutdown (`WK`) ─────────────────
logMCPDebug("alpha", "Connection failed: spawn /bin/echo ENOENT");
logMCPError("alpha", new Error("Connection failed: spawn /bin/echo ENOENT"));
logMCPDebug("beta", "Connection failed: fetch failed");
console.log("queued; tree exists before flush:", fs.existsSync(path.join(home, ".cache")));

function report() {
  for (const server of ["alpha", "beta"]) {
    const p = mcpLogPath(server);
    const exists = fs.existsSync(p);
    const records = exists ? fs.readFileSync(p, "utf8").trim().split("\n").map((l) => JSON.parse(l)) : [];
    console.log(`${server}: exists=${exists} records=${records.length}`, records.map((r) => r.debug ?? `ERR ${r.error}`).join(" | "));
  }
  console.log("recoveries:", recoveries);
  console.log("tree:", fs.existsSync(cacheRoot) ? fs.readdirSync(cacheRoot).sort().join(",") : "<missing>");
  fs.rmSync(home, { recursive: true, force: true });
}
async function gracefulShutdown(code: number) {
  let timer: any;
  try {
    await Promise.race([
      (async () => { try { await runCleanups(); } catch {} })(),
      new Promise((_resolve, reject) => { timer = setTimeout((rej: (e: Error) => void) => rej(new Error("cleanup timeout")), 2000, reject); }),
    ]);
    clearTimeout(timer);
  } catch { clearTimeout(timer); }
  report();
  process.exit(code);
}
void gracefulShutdown(0);
