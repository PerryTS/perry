# Other Modules

Additional npm packages and Node.js APIs supported by Perry. All listed here
are wired through Perry's well-known native bindings registry (#466) and
compile to native code with no JavaScript runtime involvement.

## sharp (Image Processing)

Native bindings via `perry-ext-sharp` (v0.5.551). Resizes, format conversion,
and buffer/file output all work.

```typescript,no-test
import sharp from "sharp";

const buf = await sharp("input.jpg")
  .resize(1600, 900)
  .jpeg({ quality: 80 })
  .toBuffer();

await sharp("input.png")
  .resize(300, 200)
  .toFile("output.png");
```

## cheerio (HTML Parsing)

Native bindings via `perry-ext-cheerio` (v0.5.550).

```typescript,no-test
import * as cheerio from "cheerio";

const html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
const $ = cheerio.load(html);
console.log($("h1").text()); // "Hello"
```

## nodemailer (Email)

```typescript,no-test
{{#include ../../examples/stdlib/other/snippets.ts:nodemailer}}
```

## zlib (Compression)

Native bindings via `perry-ext-zlib` (v0.5.541).

```typescript,no-test
import zlib from "zlib";

const compressed = zlib.gzipSync("Hello, World!");
const decompressed = zlib.gunzipSync(compressed);
console.log(decompressed.toString()); // "Hello, World!"
```

## cron / node-cron (Job Scheduling)

Native bindings via `perry-ext-cron` (v0.5.564). Both `cron` and `node-cron`
package names route to the same backend.

```typescript,no-test
import { CronJob } from "cron";

const job = new CronJob("*/5 * * * *", () => {
  console.log("Runs every 5 minutes");
});
job.start();
```

## ethers (Ethereum)

Native bindings via `perry-ext-ethers` (v0.5.556) — backed by
[`ethers-rs`](https://github.com/gakonst/ethers-rs)-style ABI plumbing through
`perry-ffi`'s BigInt + Buffer surfaces.

```typescript,no-test
import { ethers } from "ethers";

const wallet = ethers.Wallet.createRandom();
console.log("address:", wallet.address);
console.log("private key:", wallet.privateKey);
```

## events (EventEmitter)

Native bindings via `perry-ext-events` (v0.5.546). The `EventEmitter` shape
matches Node.js — `on`, `off`, `once`, `emit`, `removeAllListeners`.

```typescript,no-test
import { EventEmitter } from "events";

const ee = new EventEmitter();
ee.on("data", (chunk) => console.log("got:", chunk));
ee.emit("data", "hello");
```

## exponential-backoff (Retry Logic)

Native bindings via `perry-ext-exponential-backoff` (v0.5.542).

```typescript,no-test
import { backOff } from "exponential-backoff";

const result = await backOff(() => fetchUnstableEndpoint(), {
  numOfAttempts: 5,
  startingDelay: 200,
  timeMultiple: 2,
});
```

## decimal.js / bignumber.js (Arbitrary Precision)

Native bindings via `perry-ext-decimal` (v0.5.547). Both package names route
to the same backend — `Decimal` and `BigNumber` are both exposed.

```typescript,no-test
{{#include ../../examples/stdlib/other/snippets.ts:decimal}}
```

## dayjs / date-fns (Date Manipulation)

Native bindings via `perry-ext-dayjs` (v0.5.548). Both package names route to
the same Rust backend — same parse/format/diff surface.

```typescript,no-test
import dayjs from "dayjs";

const now = dayjs();
const tomorrow = now.add(1, "day");
console.log(tomorrow.format("YYYY-MM-DD"));
```

## moment (Legacy Date)

Native bindings via `perry-ext-moment` (v0.5.549). `moment` is in maintenance
mode upstream — prefer `dayjs` for new code, but Perry supports both for
existing codebases.

```typescript,no-test
import moment from "moment";

const m = moment().add(7, "days");
console.log(m.format());
```

## rate-limiter-flexible

Native bindings via `perry-ext-ratelimit` (v0.5.552). In-memory limiter is
wired; Redis / cluster backing stores are follow-ups.

```typescript,no-test
import { RateLimiterMemory } from "rate-limiter-flexible";

const limiter = new RateLimiterMemory({ points: 5, duration: 1 });
try {
  await limiter.consume("ip-1.2.3.4");
} catch (rateLimitErr) {
  console.warn("blocked:", rateLimitErr);
}
```

## worker_threads

Partially recognized at HIR-lowering time (`parentPort` / `Worker` shapes)
but full dispatch is incomplete. For data-parallel work today, prefer
`parallelMap` / `parallelFilter` / `spawn` from `perry/thread`
(see [Threading](../threading/overview.md)).

```text
import { Worker, parentPort, workerData } from "worker_threads";

if (parentPort) {
  // Worker thread
  const data = workerData;
  parentPort.postMessage({ result: data.value * 2 });
} else {
  // Main thread
  const worker = new Worker("./worker.ts", {
    workerData: { value: 21 },
  });
  worker.on("message", (msg) => {
    console.log(msg.result); // 42
  });
}
```

## commander (CLI Parsing)

```typescript,no-test
{{#include ../../examples/stdlib/other/snippets.ts:commander}}
```

## lru-cache

The wired constructor takes the npm v7+ options-object shape
(`new LRUCache({ max: 100 })`) — the older positional form
`new LRUCache(100)` falls through to a `max=100` default.

```typescript,no-test
{{#include ../../examples/stdlib/other/snippets.ts:lru-cache}}
```

## child_process

```typescript,no-test
{{#include ../../examples/stdlib/other/snippets.ts:child-process}}
```

## node-pty (Pseudo-terminals)

A runtime-native pty (#6563) — `openpty`/`fork` with the slave as the child's
controlling terminal, no N-API addon. Importable as both `node-pty` and the
API-identical `@lydell/node-pty` fork, statically or via dynamic `import()`.
POSIX only for now (macOS + Linux); on other hosts `spawn` throws, which
triggers consumers' non-pty fallback paths.

`onData` delivers UTF-8 strings (multi-byte sequences split across reads are
reassembled); `onExit` fires `{ exitCode, signal }` with the numeric signal
for a signal death and `undefined` otherwise; `kill()` defaults to `SIGHUP`
like node-pty. `TERM` in the child comes from `name`.

```typescript,no-test
import { spawn } from "node-pty";

const term = spawn("bash", [], {
  name: "xterm-256color",
  cols: 80,
  rows: 24,
  cwd: process.cwd(),
  env: process.env,
});

const sub = term.onData((data) => process.stdout.write(data));
term.onExit(({ exitCode, signal }) => {
  console.log(`exited: code=${exitCode} signal=${signal ?? "none"}`);
});

term.write("echo hello\n");
term.resize(120, 40);   // TIOCSWINSZ → child sees SIGWINCH
term.kill("SIGTERM");   // no argument = SIGHUP
sub.dispose();          // unsubscribe onData
```

## External native bindings

Two packages live in their own GitHub repos with their own semver — they're
imported by `bun add` like any npm package, but Rust-backed and compiled
natively via `perry-ffi`:

- **`@perryts/tursodb`** — Turso (libSQL fork) database client. See
  [PerryTS/tursodb-bindings](https://github.com/PerryTS/tursodb-bindings).
- **`@perryts/iroh`** — Iroh peer-to-peer networking. See
  [PerryTS/iroh-bindings](https://github.com/PerryTS/iroh-bindings).

Pure-TypeScript drivers compiled via `compilePackages`:

- **`@perryts/postgres`**, **`@perryts/mysql`**, **`@perryts/mongodb`**, **`@perryts/redis`** — wire-protocol clients that
  also run on Node.js and Bun unchanged.

See [Native Bindings — Overview](../native-libraries/overview.md) for the
contract these external packages follow.

## Next Steps

- [Overview](overview.md) — All stdlib modules
- [File System](fs.md) — fs and path APIs
- [Native Bindings](../native-libraries/overview.md) — Authoring your own
