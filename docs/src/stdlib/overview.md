# Standard Library Overview

Perry natively implements many popular npm packages and Node.js APIs. When you import a supported package, Perry compiles it to native code — no JavaScript runtime involved.

## How It Works

```typescript
{{#include ../../examples/stdlib/overview/snippets.ts:imports}}
```

Perry recognizes these imports at compile time and routes them to native
Rust implementations. Most live in standalone `perry-ext-*` crates
backed by the stable [`perry-ffi` ABI](../native-libraries/abi.md) (the
"well-known native bindings" registry shipped in v0.5.532); a few of
the older Node.js built-ins still live in `perry-stdlib`. Either way the
import surface matches the original npm package, so existing code often
works unchanged.

## Supported Packages

### Networking & HTTP
- **node:http** / **node:https** / **node:http2** — Node.js stdlib HTTP server modules + WebSocket upgrade dispatch (issue #577). The full `IncomingMessage` / `ServerResponse` surface plus TLS via rustls and HTTP/2 via ALPN. See [HTTP & Networking](http.md#nodejs-compatibility--nodehttp--nodehttps--nodehttp2).
- **hono** — runtime-agnostic web framework. `app.fetch` works end-to-end via `compilePackages` (testing + edge runtimes). Long-lived port-listening server pattern is currently blocked on [#589](https://github.com/PerryTS/perry/issues/589). See [HTTP & Networking → Hono](http.md#hono).
- **fastify** — HTTP server framework (native binding, separate from node:http).
- **axios** — HTTP client.
- **node-fetch** / **fetch** — HTTP fetch API.
- **ws** — WebSocket client/server.

### Databases
- **mysql2** — MySQL client
- **pg** — PostgreSQL client
- **better-sqlite3** — SQLite
- **mongodb** — MongoDB client
- **ioredis** / **redis** — Redis client

### Cryptography
- **bcrypt** — Password hashing
- **argon2** — Password hashing (Argon2)
- **jsonwebtoken** — JWT signing/verification
- **crypto** — Node.js crypto module
- **ethers** — Ethereum library

### Utilities
- **lodash** — Utility functions
- **dayjs** / **moment** — Date manipulation
- **uuid** — UUID generation
- **nanoid** — ID generation
- **slugify** — String slugification
- **validator** — String validation

### CLI & Data
- **commander** — CLI argument parsing
- **decimal.js** — Arbitrary precision decimals
- **bignumber.js** — Big number math
- **lru-cache** — LRU caching

### Other
- **sharp** — Image processing
- **cheerio** — HTML parsing
- **nodemailer** — Email sending
- **zlib** — Compression
- **cron** — Job scheduling
- **worker_threads** — Background workers
- **exponential-backoff** — Retry logic
- **async_hooks** — AsyncLocalStorage
- **perry/container** — OCI container management
- **perry/compose** — Multi-container orchestration

### Node.js Built-ins
- **fs** — File system
- **path** — Path manipulation
- **child_process** — Process spawning
- **crypto** — Cryptographic functions

## Binary Size

Perry automatically detects which stdlib features your code uses:

| Usage | Binary Size |
|-------|-------------|
| No stdlib imports | ~300KB |
| fs + path only | ~3MB |
| Full stdlib | ~48MB |

The compiler links only the required runtime components.

### External native bindings

Two packages live in their own GitHub repos with their own semver but
plug into the same well-known registry:

- **`@perryts/tursodb`** — Turso (libSQL fork) database client.
  [PerryTS/tursodb-bindings](https://github.com/PerryTS/tursodb-bindings).
- **`@perryts/iroh`** — Iroh peer-to-peer networking.
  [PerryTS/iroh-bindings](https://github.com/PerryTS/iroh-bindings).

Pure-TypeScript drivers compiled via `compilePackages` (no Rust):

- **`@perryts/postgres`** — pg-compatible wire-protocol driver.
- **`@perryts/mysql`** — mysql2-compatible wire-protocol driver.
- **`@perryts/mongodb`** — mongodb-compatible wire-protocol driver.
- **`@perryts/redis`** — Redis / Valkey RESP2 + RESP3 wire-protocol driver.

Each of these also runs unmodified on Node.js / Bun. See
[Native Bindings — Overview](../native-libraries/overview.md) for
the contract they follow.

## compilePackages

For npm packages not natively supported, you can compile pure TypeScript/JavaScript packages natively:

```json
{
  "perry": {
    "compilePackages": ["@noble/curves", "@noble/hashes"]
  }
}
```

See [Project Configuration](../getting-started/project-config.md) for details.

## JavaScript Runtime Fallback

For packages that can't be compiled natively (native addons, dynamic code, etc.), Perry includes a QuickJS-based JavaScript runtime as a fallback. The exact API surface is internal-only today; the import below is illustrative:

```text
import { jsEval } from "perry/jsruntime"; // illustrative — not yet a public export
```

## Next Steps

- [File System](fs.md)
- [HTTP & Networking](http.md)
- [Databases](database.md)
- [Cryptography](crypto.md)
- [Containers](container.md)
- [Utilities](utilities.md)
- [Other Modules](other.md)
