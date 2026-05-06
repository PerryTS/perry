# Supported API Reference

This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.

**Generated for Perry v0.5.598.**

Total: 397 entries across 45 modules.

## Modules

- [`argon2`](#argon2)
- [`async_hooks`](#async-hooks)
- [`bcrypt`](#bcrypt)
- [`better-sqlite3`](#better-sqlite3)
- [`buffer`](#buffer)
- [`cheerio`](#cheerio)
- [`commander`](#commander)
- [`cron`](#cron)
- [`crypto`](#crypto)
- [`dayjs`](#dayjs)
- [`decimal.js`](#decimal-js)
- [`dotenv`](#dotenv)
- [`ethers`](#ethers)
- [`events`](#events)
- [`exponential-backoff`](#exponential-backoff)
- [`fastify`](#fastify)
- [`ioredis`](#ioredis)
- [`iroh`](#iroh)
- [`jsonwebtoken`](#jsonwebtoken)
- [`lodash`](#lodash)
- [`lru-cache`](#lru-cache)
- [`moment`](#moment)
- [`mongodb`](#mongodb)
- [`mysql2`](#mysql2)
- [`mysql2/promise`](#mysql2-promise)
- [`nanoid`](#nanoid)
- [`net`](#net)
- [`nodemailer`](#nodemailer)
- [`os`](#os)
- [`path`](#path)
- [`perry/thread`](#perry-thread)
- [`perry/tui`](#perry-tui)
- [`pg`](#pg)
- [`process`](#process)
- [`readline`](#readline)
- [`sharp`](#sharp)
- [`slugify`](#slugify)
- [`tls`](#tls)
- [`tursodb`](#tursodb)
- [`url`](#url)
- [`uuid`](#uuid)
- [`validator`](#validator)
- [`worker_threads`](#worker-threads)
- [`ws`](#ws)
- [`zlib`](#zlib)

---

## `argon2`

### Methods

- `hash` — module
- `verify` — module

## `async_hooks`

### Methods

- `disable` — instance
- `enterWith` — instance
- `exit` — instance
- `getStore` — instance
- `run` — instance

## `bcrypt`

### Methods

- `compare` — module
- `hash` — module

## `better-sqlite3`

### Methods

- `all` — instance
- `close` — instance
- `default` — module
- `exec` — instance
- `get` — instance
- `prepare` — instance
- `run` — instance

## `buffer`

### Classes

- `Buffer`

## `cheerio`

### Methods

- `attr` — instance
- `children` — instance
- `eq` — instance
- `find` — instance
- `first` — instance
- `hasClass` — instance
- `html` — instance
- `last` — instance
- `length` — instance
- `load` — module
- `parent` — instance
- `select` — instance
- `text` — instance

## `commander`

### Methods

- `action` — instance
- `command` — instance
- `description` — instance
- `name` — instance
- `option` — instance
- `opts` — instance
- `parse` — instance
- `requiredOption` — instance
- `version` — instance

## `cron`

### Methods

- `describe` — module
- `isRunning` — instance
- `nextDate` — instance
- `schedule` — module
- `start` — instance
- `stop` — instance
- `validate` — module

## `crypto`

### Methods

- `createHash` — module
- `createHmac` — module
- `getRandomValues` — module
- `md5` — module
- `pbkdf2` — module
- `pbkdf2Sync` — module
- `randomBytes` — module
- `randomUUID` — module
- `sha256` — module

## `dayjs`

### Methods

- `add` — instance
- `clone` — instance
- `date` — instance
- `day` — instance
- `dayjs` — module
- `default` — module
- `diff` — instance
- `endOf` — instance
- `format` — instance
- `hour` — instance
- `isAfter` — instance
- `isBefore` — instance
- `isSame` — instance
- `isValid` — instance
- `millisecond` — instance
- `minute` — instance
- `month` — instance
- `second` — instance
- `startOf` — instance
- `subtract` — instance
- `toISOString` — instance
- `unix` — instance
- `valueOf` — instance
- `year` — instance

## `decimal.js`

### Methods

- `abs` — instance
- `ceil` — instance
- `cmp` — instance
- `div` — instance
- `eq` — instance
- `floor` — instance
- `gt` — instance
- `gte` — instance
- `isNegative` — instance
- `isPositive` — instance
- `isZero` — instance
- `lt` — instance
- `lte` — instance
- `minus` — instance
- `mod` — instance
- `neg` — instance
- `plus` — instance
- `pow` — instance
- `round` — instance
- `sqrt` — instance
- `times` — instance
- `toFixed` — instance
- `toNumber` — instance
- `toString` — instance
- `valueOf` — instance

## `dotenv`

### Methods

- `config` — module

## `ethers`

### Methods

- `createRandom` — module *(class: `Wallet`)*
- `formatEther` — module
- `formatUnits` — module
- `getAddress` — module
- `parseEther` — module
- `parseUnits` — module

## `events`

### Classes

- `EventEmitter`

### Methods

- `EventEmitter` — module
- `emit` — instance
- `on` — instance
- `removeAllListeners` — instance
- `removeListener` — instance

## `exponential-backoff`

### Methods

- `backOff` — module

## `fastify`

### Methods

- `addHook` — instance
- `all` — instance
- `body` — instance
- `code` — instance
- `default` — module
- `delete` — instance
- `get` — instance
- `head` — instance
- `header` — instance
- `headers` — instance
- `html` — instance
- `json` — instance
- `listen` — instance
- `method` — instance
- `options` — instance
- `param` — instance
- `params` — instance
- `patch` — instance
- `post` — instance
- `put` — instance
- `query` — instance
- `rawBody` — instance
- `redirect` — instance
- `register` — instance
- `route` — instance
- `send` — instance
- `setErrorHandler` — instance
- `status` — instance
- `text` — instance
- `url` — instance
- `user` — instance

## `ioredis`

### Classes

- `Redis`

### Methods

- `createClient` — module
- `decr` — instance
- `del` — instance
- `exists` — instance
- `expire` — instance
- `get` — instance
- `incr` — instance
- `quit` — instance
- `set` — instance

## `iroh`

### Methods

- `acceptBi` — instance
- `acceptOne` — instance
- `bind` — module
- `close` — instance
- `connClose` — instance
- `connect` — instance
- `nodeId` — instance
- `openBi` — instance
- `streamFinish` — instance
- `streamReadToEnd` — instance
- `streamWrite` — instance

## `jsonwebtoken`

### Methods

- `decode` — module
- `sign` — module
- `verify` — module

## `lodash`

### Methods

- `camelCase` — module
- `chunk` — module
- `clamp` — module
- `compact` — module
- `drop` — module
- `first` — module
- `flatten` — module
- `head` — module
- `kebabCase` — module
- `last` — module
- `range` — module
- `reverse` — module
- `size` — module
- `snakeCase` — module
- `take` — module
- `times` — module
- `uniq` — module

## `lru-cache`

### Methods

- `clear` — instance
- `default` — module
- `delete` — instance
- `get` — instance
- `has` — instance
- `set` — instance
- `size` — instance

## `moment`

### Methods

- `default` — module
- `moment` — module

## `mongodb`

### Methods

- `close` — instance
- `collection` — instance
- `connect` — module
- `connect` — instance
- `countDocuments` — instance
- `db` — instance
- `deleteMany` — instance
- `deleteOne` — instance
- `find` — instance
- `findOne` — instance
- `insertMany` — instance
- `insertOne` — instance
- `updateMany` — instance
- `updateOne` — instance

## `mysql2`

### Classes

- `Pool`

### Methods

- `beginTransaction` — instance
- `commit` — instance
- `createConnection` — module
- `createPool` — module
- `end` — instance *(class: `Pool`)*
- `end` — instance
- `execute` — instance *(class: `Pool`)*
- `execute` — instance *(class: `PoolConnection`)*
- `execute` — instance
- `getConnection` — instance
- `query` — instance *(class: `Pool`)*
- `query` — instance *(class: `PoolConnection`)*
- `query` — instance
- `release` — instance
- `rollback` — instance

## `mysql2/promise`

### Classes

- `Pool`

### Methods

- `beginTransaction` — instance
- `commit` — instance
- `createConnection` — module
- `createPool` — module
- `end` — instance *(class: `Pool`)*
- `end` — instance
- `execute` — instance *(class: `Pool`)*
- `execute` — instance *(class: `PoolConnection`)*
- `execute` — instance
- `getConnection` — instance
- `query` — instance *(class: `Pool`)*
- `query` — instance *(class: `PoolConnection`)*
- `query` — instance
- `release` — instance
- `rollback` — instance

## `nanoid`

### Methods

- `nanoid` — module

## `net`

### Classes

- `Server`
- `Socket`

### Methods

- `Socket` — module
- `connect` — module
- `connect` — instance *(class: `Socket`)*
- `createConnection` — module
- `destroy` — instance *(class: `Socket`)*
- `end` — instance *(class: `Socket`)*
- `on` — instance *(class: `Socket`)*
- `upgradeToTLS` — instance *(class: `Socket`)*
- `write` — instance *(class: `Socket`)*

## `nodemailer`

### Methods

- `createTransport` — module
- `sendMail` — instance
- `verify` — instance

## `os`

### Methods

- `arch` — module
- `cpus` — module
- `freemem` — module
- `homedir` — module
- `hostname` — module
- `networkInterfaces` — module
- `platform` — module
- `release` — module
- `tmpdir` — module
- `totalmem` — module
- `type` — module
- `uptime` — module
- `userInfo` — module

### Properties

- `EOL`

## `path`

### Methods

- `basename` — module
- `dirname` — module
- `extname` — module
- `format` — module
- `isAbsolute` — module
- `join` — module
- `normalize` — module
- `parse` — module
- `relative` — module
- `resolve` — module

### Properties

- `delimiter`
- `posix`
- `sep`
- `win32`

## `perry/thread`

### Methods

- `parallelFilter` — module
- `parallelMap` — module
- `spawn` — module

## `perry/tui`

### Methods

- `Box` — module
- `Input` — module
- `List` — module
- `ProgressBar` — module
- `Select` — module
- `Spacer` — module
- `Spinner` — module
- `Text` — module
- `TextArea` — module
- `boxSetAlignItems` — module
- `boxSetFlexDirection` — module
- `boxSetFlexGrow` — module
- `boxSetGap` — module
- `boxSetHeight` — module
- `boxSetJustifyContent` — module
- `boxSetPadding` — module
- `boxSetWidth` — module
- `enter` — module
- `exit` — module
- `get` — instance *(class: `State`)*
- `render` — module
- `run` — module
- `set` — instance *(class: `State`)*
- `state` — module
- `useInput` — module

## `pg`

### Classes

- `Client`
- `Pool`

### Methods

- `Pool` — module
- `connect` — module
- `connect` — instance *(class: `Client`)*
- `end` — instance *(class: `Pool`)*
- `end` — instance
- `query` — instance *(class: `Pool`)*
- `query` — instance

## `process`

### Properties

- `arch`
- `argv`
- `env`
- `pid`
- `platform`
- `ppid`
- `stderr`
- `stdin`
- `stdout`
- `version`
- `versions`

## `readline`

### Methods

- `close` — instance
- `createInterface` — module
- `on` — instance
- `question` — instance

## `sharp`

### Methods

- `blur` — instance
- `default` — module
- `flip` — instance
- `flop` — instance
- `grayscale` — instance
- `height` — instance
- `jpeg` — instance
- `metadata` — instance
- `png` — instance
- `resize` — instance
- `rotate` — instance
- `sharp` — module
- `toBuffer` — instance
- `toFile` — instance
- `webp` — instance
- `width` — instance

## `slugify`

### Methods

- `default` — module
- `slugify` — module

## `tls`

### Methods

- `connect` — module

## `tursodb`

### Methods

- `close` — instance
- `exec` — instance
- `execBatch` — instance
- `isAutocommit` — instance
- `lastInsertRowid` — instance
- `open` — module
- `queryAll` — instance
- `queryOne` — instance

## `url`

### Classes

- `URL`
- `URLSearchParams`

## `uuid`

### Methods

- `v1` — module
- `v4` — module
- `v7` — module
- `validate` — module

## `validator`

### Methods

- `isEmail` — module
- `isEmpty` — module
- `isJSON` — module
- `isURL` — module
- `isUUID` — module

## `worker_threads`

### Methods

- `getWorkerData` — module
- `parentPort` — module
- `postMessage` — instance
- `workerData` — module

## `ws`

### Classes

- `WebSocket`
- `WebSocketServer`

### Methods

- `Server` — module
- `WebSocket` — module
- `close` — instance
- `closeClient` — module
- `on` — instance
- `send` — instance
- `sendToClient` — module

## `zlib`

### Methods

- `deflateSync` — module
- `gunzip` — module
- `gunzipSync` — module
- `gzip` — module
- `gzipSync` — module
- `inflateSync` — module

