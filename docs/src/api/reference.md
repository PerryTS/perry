# Supported API Reference

This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.

Total: 1481 entries across 82 modules.

## Modules

- [`@perryts/pdf`](#-perryts-pdf)
- [`argon2`](#argon2)
- [`assert`](#assert)
- [`assert/strict`](#assert-strict)
- [`async_hooks`](#async-hooks)
- [`axios`](#axios)
- [`bcrypt`](#bcrypt)
- [`better-sqlite3`](#better-sqlite3)
- [`bignumber.js`](#bignumber-js)
- [`buffer`](#buffer)
- [`cheerio`](#cheerio)
- [`child_process`](#child-process)
- [`cluster`](#cluster)
- [`commander`](#commander)
- [`console`](#console)
- [`cron`](#cron)
- [`crypto`](#crypto)
- [`date-fns`](#date-fns)
- [`dayjs`](#dayjs)
- [`decimal.js`](#decimal-js)
- [`dotenv`](#dotenv)
- [`ethers`](#ethers)
- [`events`](#events)
- [`exponential-backoff`](#exponential-backoff)
- [`fastify`](#fastify)
- [`fetch`](#fetch)
- [`fs`](#fs)
- [`http`](#http)
- [`http2`](#http2)
- [`https`](#https)
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
- [`node-cron`](#node-cron)
- [`node-fetch`](#node-fetch)
- [`nodemailer`](#nodemailer)
- [`os`](#os)
- [`path`](#path)
- [`perf_hooks`](#perf-hooks)
- [`perry/ads`](#perry-ads)
- [`perry/audio`](#perry-audio)
- [`perry/background`](#perry-background)
- [`perry/i18n`](#perry-i18n)
- [`perry/media`](#perry-media)
- [`perry/plugin`](#perry-plugin)
- [`perry/system`](#perry-system)
- [`perry/thread`](#perry-thread)
- [`perry/tui`](#perry-tui)
- [`perry/ui`](#perry-ui)
- [`perry/updater`](#perry-updater)
- [`perry/widget`](#perry-widget)
- [`pg`](#pg)
- [`process`](#process)
- [`querystring`](#querystring)
- [`rate-limiter-flexible`](#rate-limiter-flexible)
- [`readline`](#readline)
- [`redis`](#redis)
- [`sharp`](#sharp)
- [`slugify`](#slugify)
- [`stream`](#stream)
- [`stream/promises`](#stream-promises)
- [`streams`](#streams)
- [`string_decoder`](#string-decoder)
- [`tls`](#tls)
- [`tty`](#tty)
- [`tursodb`](#tursodb)
- [`url`](#url)
- [`util`](#util)
- [`util/types`](#util-types)
- [`uuid`](#uuid)
- [`validator`](#validator)
- [`worker_threads`](#worker-threads)
- [`ws`](#ws)
- [`zlib`](#zlib)

---

## `@perryts/pdf`

### Methods

- `createPdf` — module
- `pdfAddLine` — module
- `pdfAddText` — module
- `pdfNewPage` — module
- `pdfSave` — module

## `argon2`

### Methods

- `hash` — module
- `verify` — module

## `assert`

### Classes

- `AssertionError`

### Methods

- `deepEqual` — module
- `deepStrictEqual` — module
- `default` — module
- `doesNotMatch` — module
- `doesNotReject` — module
- `doesNotThrow` — module
- `equal` — module
- `fail` — module
- `ifError` — module
- `match` — module
- `notDeepEqual` — module
- `notDeepStrictEqual` — module
- `notEqual` — module
- `notStrictEqual` — module
- `ok` — module
- `rejects` — module
- `strict` — module
- `strictEqual` — module
- `throws` — module

### Properties

- `strict`

## `assert/strict`

### Classes

- `AssertionError`

### Methods

- `deepEqual` — module
- `deepStrictEqual` — module
- `default` — module
- `doesNotMatch` — module
- `doesNotReject` — module
- `doesNotThrow` — module
- `equal` — module
- `fail` — module
- `ifError` — module
- `match` — module
- `notDeepEqual` — module
- `notDeepStrictEqual` — module
- `notEqual` — module
- `notStrictEqual` — module
- `ok` — module
- `rejects` — module
- `strictEqual` — module
- `throws` — module

## `async_hooks`

### Classes

- `AsyncLocalStorage`
- `AsyncResource`

### Methods

- `asyncId` — instance *(class: `AsyncResource`)*
- `bind` — instance *(class: `AsyncResource`)*
- `createHook` — module
- `disable` — instance
- `emitDestroy` — instance *(class: `AsyncResource`)*
- `enable` — instance *(class: `AsyncHook`)*
- `enterWith` — instance
- `executionAsyncId` — module
- `exit` — instance
- `getStore` — instance
- `run` — instance
- `runInAsyncScope` — instance *(class: `AsyncResource`)*
- `triggerAsyncId` — module
- `triggerAsyncId` — instance *(class: `AsyncResource`)*

## `axios`

### Methods

- `all` — module
- `create` — module
- `default` — module
- `delete` — module
- `get` — module
- `head` — module
- `options` — module
- `patch` — module
- `post` — module
- `put` — module
- `request` — module

## `bcrypt`

### Methods

- `compare` — module
- `hash` — module

## `better-sqlite3`

### Methods

- `all` — instance
- `close` — instance
- `columns` — instance
- `default` — module
- `exec` — instance
- `get` — instance
- `iterate` — instance
- `pluck` — instance
- `pragma` — instance
- `prepare` — instance
- `raw` — instance
- `run` — instance
- `transaction` — instance

## `bignumber.js`

### Classes

- `BigNumber`

## `buffer`

### Classes

- `Blob`
- `Buffer`
- `File`

### Methods

- `alloc` — module
- `allocUnsafe` — module
- `allocUnsafeSlow` — module
- `byteLength` — module
- `concat` — module
- `from` — module
- `isAscii` — module
- `isBuffer` — module
- `isEncoding` — module
- `isUtf8` — module
- `of` — module
- `resolveObjectURL` — module
- `transcode` — module

### Properties

- `constants`
- `kMaxLength`
- `kStringMaxLength`

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

## `child_process`

### Classes

- `ChildProcess`

### Methods

- `exec` — module
- `execFile` — module
- `execFileSync` — module
- `execSync` — module
- `fork` — module
- `spawn` — module
- `spawnSync` — module

### Properties

- `Stream`

## `cluster`

### Classes

- `Worker`

### Methods

- `disconnect` — module
- `fork` — module
- `setupMaster` — module
- `setupPrimary` — module

### Properties

- `SCHED_NONE`
- `SCHED_RR`
- `addListener`
- `isMaster`
- `isPrimary`
- `isWorker`
- `on`
- `schedulingPolicy`
- `settings`
- `worker`
- `workers`

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

## `console`

### Classes

- `Console`

### Methods

- `assert` — module
- `clear` — module
- `count` — module
- `countReset` — module
- `debug` — module
- `dir` — module
- `dirxml` — module
- `error` — module
- `group` — module
- `groupCollapsed` — module
- `groupEnd` — module
- `info` — module
- `log` — module
- `profile` — module
- `profileEnd` — module
- `table` — module
- `time` — module
- `timeEnd` — module
- `timeLog` — module
- `timeStamp` — module
- `trace` — module
- `warn` — module

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

### Classes

- `ECDH`
- `X509Certificate`

### Methods

- `createCipheriv` — module
- `createDecipheriv` — module
- `createDiffieHellman` — module
- `createDiffieHellmanGroup` — module
- `createECDH` — module
- `createHash` — module
- `createHmac` — module
- `createPrivateKey` — module
- `createPublicKey` — module
- `createSecretKey` — module
- `createSign` — module
- `createSign` — module
- `createVerify` — module
- `createVerify` — module
- `generateKeyPairSync` — module
- `generateKeyPairSync` — module
- `getCiphers` — module
- `getCurves` — module
- `getDiffieHellman` — module
- `getFips` — module
- `getHashes` — module
- `getRandomValues` — module
- `hash` — module
- `hkdfSync` — module
- `md5` — module
- `pbkdf2` — module
- `pbkdf2Sync` — module
- `privateDecrypt` — module
- `privateEncrypt` — module
- `publicDecrypt` — module
- `publicEncrypt` — module
- `randomBytes` — module
- `randomFillSync` — module
- `randomInt` — module
- `randomInt` — module
- `randomUUID` — module
- `scryptSync` — module
- `sha256` — module
- `sign` — module
- `timingSafeEqual` — module
- `verify` — module

### Properties

- `Certificate`
- `constants`
- `subtle`

## `date-fns`

### Methods

- `addDays` — module
- `addMonths` — module
- `addYears` — module
- `differenceInDays` — module
- `differenceInHours` — module
- `differenceInMinutes` — module
- `endOfDay` — module
- `format` — module
- `isAfter` — module
- `isBefore` — module
- `parseISO` — module
- `startOfDay` — module

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
- `addAbortListener` — module
- `addListener` — instance
- `emit` — instance
- `eventNames` — instance
- `getEventListeners` — module
- `getMaxListeners` — instance
- `getMaxListeners` — module
- `listenerCount` — instance
- `listenerCount` — module
- `listeners` — instance
- `off` — instance
- `on` — instance
- `on` — module
- `once` — instance
- `once` — module
- `prependListener` — instance
- `prependOnceListener` — instance
- `rawListeners` — instance
- `removeAllListeners` — instance
- `removeListener` — instance
- `setMaxListeners` — instance
- `setMaxListeners` — module

### Properties

- `captureRejectionSymbol`
- `captureRejections`
- `defaultMaxListeners`
- `errorMonitor`

## `exponential-backoff`

### Methods

- `backOff` — module

## `fastify`

### Methods

- `addHook` — instance
- `all` — instance
- `body` — instance
- `close` — instance
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
- `on` — instance
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
- `server` — instance
- `setErrorHandler` — instance
- `status` — instance
- `text` — instance
- `type` — instance
- `url` — instance
- `user` — instance

## `fetch`

### Classes

- `Blob`
- `Headers`
- `Request`
- `Response`

### Methods

- `default` — module

## `fs`

### Methods

- `access` — module
- `accessSync` — module
- `appendFile` — module
- `appendFileSync` — module
- `chmod` — module
- `chmodSync` — module
- `chown` — module
- `chownSync` — module
- `close` — module
- `closeSync` — module
- `copyFile` — module
- `copyFileSync` — module
- `cp` — module
- `cpSync` — module
- `createReadStream` — module
- `createWriteStream` — module
- `exists` — module
- `existsSync` — module
- `fchmod` — module
- `fchmodSync` — module
- `fchown` — module
- `fchownSync` — module
- `fdatasync` — module
- `fdatasyncSync` — module
- `fstat` — module
- `fstatSync` — module
- `fsync` — module
- `fsyncSync` — module
- `ftruncate` — module
- `ftruncateSync` — module
- `futimes` — module
- `futimesSync` — module
- `glob` — module
- `globSync` — module
- `lchmod` — module
- `lchmodSync` — module
- `lchown` — module
- `lchownSync` — module
- `link` — module
- `linkSync` — module
- `lstat` — module
- `lstatSync` — module
- `lutimes` — module
- `lutimesSync` — module
- `mkdir` — module
- `mkdirSync` — module
- `mkdtemp` — module
- `mkdtempSync` — module
- `open` — module
- `openSync` — module
- `opendir` — module
- `opendirSync` — module
- `read` — module
- `readFile` — module
- `readFileSync` — module
- `readSync` — module
- `readdir` — module
- `readdirSync` — module
- `readlink` — module
- `readlinkSync` — module
- `readv` — module
- `readvSync` — module
- `realpath` — module
- `realpathSync` — module
- `rename` — module
- `renameSync` — module
- `rm` — module
- `rmSync` — module
- `rmdir` — module
- `rmdirSync` — module
- `stat` — module
- `statSync` — module
- `statfs` — module
- `statfsSync` — module
- `symlink` — module
- `symlinkSync` — module
- `truncate` — module
- `truncateSync` — module
- `unlink` — module
- `unlinkSync` — module
- `unwatchFile` — module
- `utimes` — module
- `utimesSync` — module
- `watch` — module
- `watchFile` — module
- `write` — module
- `writeFile` — module
- `writeFileSync` — module
- `writeSync` — module
- `writev` — module
- `writevSync` — module

### Properties

- `constants`
- `promises`

## `http`

### Classes

- `Agent`
- `ClientRequest`
- `IncomingMessage`
- `IncomingMessage`
- `Server`
- `Server`
- `ServerResponse`
- `ServerResponse`

### Methods

- `Agent` — module
- `Server` — module
- `__get_aborted` — instance *(class: `IncomingMessage`)*
- `__get_complete` — instance *(class: `IncomingMessage`)*
- `__get_createConnection` — instance *(class: `Agent`)*
- `__get_createSocket` — instance *(class: `Agent`)*
- `__get_destroyed` — instance *(class: `Agent`)*
- `__get_destroyed` — instance *(class: `IncomingMessage`)*
- `__get_freeSockets` — instance *(class: `Agent`)*
- `__get_headers` — instance *(class: `IncomingMessage`)*
- `__get_headersSent` — instance *(class: `ServerResponse`)*
- `__get_headersTimeout` — instance *(class: `HttpServer`)*
- `__get_httpVersion` — instance *(class: `IncomingMessage`)*
- `__get_keepAlive` — instance *(class: `Agent`)*
- `__get_keepAliveMsecs` — instance *(class: `Agent`)*
- `__get_keepAliveTimeout` — instance *(class: `HttpServer`)*
- `__get_maxFreeSockets` — instance *(class: `Agent`)*
- `__get_maxHeadersCount` — instance *(class: `HttpServer`)*
- `__get_maxRequestsPerSocket` — instance *(class: `HttpServer`)*
- `__get_maxSockets` — instance *(class: `Agent`)*
- `__get_maxTotalSockets` — instance *(class: `Agent`)*
- `__get_method` — instance *(class: `IncomingMessage`)*
- `__get_protocol` — instance *(class: `Agent`)*
- `__get_requestTimeout` — instance *(class: `HttpServer`)*
- `__get_requests` — instance *(class: `Agent`)*
- `__get_sockets` — instance *(class: `Agent`)*
- `__get_statusCode` — instance *(class: `IncomingMessage`)*
- `__get_statusCode` — instance *(class: `ServerResponse`)*
- `__get_statusMessage` — instance *(class: `IncomingMessage`)*
- `__get_timeout` — instance *(class: `HttpServer`)*
- `__get_trailers` — instance *(class: `IncomingMessage`)*
- `__get_url` — instance *(class: `IncomingMessage`)*
- `__get_writableEnded` — instance *(class: `ServerResponse`)*
- `__get_writableFinished` — instance *(class: `ServerResponse`)*
- `__set_createConnection` — instance *(class: `Agent`)*
- `__set_createSocket` — instance *(class: `Agent`)*
- `__set_headersTimeout` — instance *(class: `HttpServer`)*
- `__set_keepAlive` — instance *(class: `Agent`)*
- `__set_keepAliveMsecs` — instance *(class: `Agent`)*
- `__set_keepAliveTimeout` — instance *(class: `HttpServer`)*
- `__set_maxFreeSockets` — instance *(class: `Agent`)*
- `__set_maxHeadersCount` — instance *(class: `HttpServer`)*
- `__set_maxRequestsPerSocket` — instance *(class: `HttpServer`)*
- `__set_maxSockets` — instance *(class: `Agent`)*
- `__set_maxTotalSockets` — instance *(class: `Agent`)*
- `__set_protocol` — instance *(class: `Agent`)*
- `__set_requestTimeout` — instance *(class: `HttpServer`)*
- `__set_statusCode` — instance *(class: `ServerResponse`)*
- `__set_statusMessage` — instance *(class: `ServerResponse`)*
- `__set_timeout` — instance *(class: `HttpServer`)*
- `addListener` — instance *(class: `HttpServer`)*
- `addListener` — instance *(class: `IncomingMessage`)*
- `addListener` — instance *(class: `ServerResponse`)*
- `addTrailers` — instance *(class: `ServerResponse`)*
- `address` — instance *(class: `HttpServer`)*
- `close` — instance *(class: `Agent`)*
- `close` — instance *(class: `HttpServer`)*
- `closeAllConnections` — instance *(class: `HttpServer`)*
- `closeIdleConnections` — instance *(class: `HttpServer`)*
- `createServer` — module
- `createServer` — module
- `destroy` — instance *(class: `Agent`)*
- `destroy` — instance *(class: `IncomingMessage`)*
- `destroyed` — instance *(class: `Agent`)*
- `end` — instance *(class: `ServerResponse`)*
- `flushHeaders` — instance *(class: `ServerResponse`)*
- `freeSockets` — instance *(class: `Agent`)*
- `get` — module
- `getHeader` — instance *(class: `ServerResponse`)*
- `getName` — instance *(class: `Agent`)*
- `getStatus` — instance *(class: `ServerResponse`)*
- `hasHeader` — instance *(class: `ServerResponse`)*
- `headers` — instance *(class: `IncomingMessage`)*
- `headersTimeout` — instance *(class: `HttpServer`)*
- `httpVersion` — instance *(class: `IncomingMessage`)*
- `keepAlive` — instance *(class: `Agent`)*
- `keepAliveMsecs` — instance *(class: `Agent`)*
- `keepAliveTimeout` — instance *(class: `HttpServer`)*
- `keepSocketAlive` — instance *(class: `Agent`)*
- `listen` — instance *(class: `HttpServer`)*
- `maxFreeSockets` — instance *(class: `Agent`)*
- `maxHeadersCount` — instance *(class: `HttpServer`)*
- `maxRequestsPerSocket` — instance *(class: `HttpServer`)*
- `maxSockets` — instance *(class: `Agent`)*
- `maxTotalSockets` — instance *(class: `Agent`)*
- `method` — instance *(class: `IncomingMessage`)*
- `on` — instance *(class: `HttpServer`)*
- `on` — instance *(class: `IncomingMessage`)*
- `on` — instance *(class: `ServerResponse`)*
- `pause` — instance *(class: `IncomingMessage`)*
- `protocol` — instance *(class: `Agent`)*
- `read` — instance *(class: `IncomingMessage`)*
- `removeHeader` — instance *(class: `ServerResponse`)*
- `request` — module
- `requestTimeout` — instance *(class: `HttpServer`)*
- `requests` — instance *(class: `Agent`)*
- `resume` — instance *(class: `IncomingMessage`)*
- `reuseSocket` — instance *(class: `Agent`)*
- `setHeader` — instance *(class: `ServerResponse`)*
- `setStatus` — instance *(class: `ServerResponse`)*
- `setTimeout` — instance *(class: `HttpServer`)*
- `setTimeout` — instance *(class: `ClientRequest`)*
- `sockets` — instance *(class: `Agent`)*
- `statusCode` — instance *(class: `IncomingMessage`)*
- `statusMessage` — instance *(class: `IncomingMessage`)*
- `timeout` — instance *(class: `HttpServer`)*
- `trailers` — instance *(class: `IncomingMessage`)*
- `url` — instance *(class: `IncomingMessage`)*
- `write` — instance *(class: `ServerResponse`)*
- `writeContinue` — instance *(class: `ServerResponse`)*
- `writeHead` — instance *(class: `ServerResponse`)*
- `writeProcessing` — instance *(class: `ServerResponse`)*

### Properties

- `METHODS`
- `STATUS_CODES`

## `http2`

### Classes

- `Http2SecureServer`
- `Http2ServerRequest`
- `Http2ServerResponse`

### Methods

- `address` — instance *(class: `Http2SecureServer`)*
- `close` — instance *(class: `Http2SecureServer`)*
- `createSecureServer` — module
- `listen` — instance *(class: `Http2SecureServer`)*
- `on` — instance *(class: `Http2SecureServer`)*

### Properties

- `constants`

## `https`

### Classes

- `Agent`
- `ClientRequest`
- `IncomingMessage`
- `Server`
- `Server`
- `ServerResponse`

### Methods

- `Agent` — module
- `Server` — module
- `address` — instance *(class: `HttpsServer`)*
- `close` — instance *(class: `HttpsServer`)*
- `createServer` — module
- `createServer` — module
- `get` — module
- `listen` — instance *(class: `HttpsServer`)*
- `on` — instance *(class: `HttpsServer`)*
- `request` — module

## `ioredis`

### Classes

- `Redis`

### Methods

- `connect` — instance
- `createClient` — module
- `decr` — instance
- `del` — instance
- `disconnect` — instance
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
- `clamp` — module
- `compact` — module
- `drop` — module
- `first` — module
- `flatten` — module
- `head` — module
- `inRange` — module
- `kebabCase` — module
- `last` — module
- `max` — module
- `maxBy` — module
- `mean` — module
- `meanBy` — module
- `min` — module
- `minBy` — module
- `random` — module
- `range` — module
- `reverse` — module
- `size` — module
- `snakeCase` — module
- `sum` — module
- `sumBy` — module
- `tail` — module
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
- `addListener` — instance *(class: `Socket`)*
- `addListener` — instance *(class: `Server`)*
- `address` — instance *(class: `Socket`)*
- `address` — instance *(class: `Server`)*
- `close` — instance *(class: `Server`)*
- `connect` — module
- `connect` — instance *(class: `Socket`)*
- `cork` — instance *(class: `Socket`)*
- `createConnection` — module
- `destroy` — instance *(class: `Socket`)*
- `end` — instance *(class: `Socket`)*
- `eventNames` — instance *(class: `Socket`)*
- `eventNames` — instance *(class: `Server`)*
- `getDefaultAutoSelectFamily` — module
- `getDefaultAutoSelectFamilyAttemptTimeout` — module
- `isIP` — module
- `isIPv4` — module
- `isIPv6` — module
- `listen` — instance *(class: `Server`)*
- `listenerCount` — instance *(class: `Socket`)*
- `listenerCount` — instance *(class: `Server`)*
- `listeners` — instance *(class: `Socket`)*
- `listeners` — instance *(class: `Server`)*
- `off` — instance *(class: `Socket`)*
- `off` — instance *(class: `Server`)*
- `on` — instance *(class: `Socket`)*
- `once` — instance *(class: `Socket`)*
- `once` — instance *(class: `Server`)*
- `pause` — instance *(class: `Socket`)*
- `rawListeners` — instance *(class: `Socket`)*
- `rawListeners` — instance *(class: `Server`)*
- `ref` — instance *(class: `Socket`)*
- `removeAllListeners` — instance *(class: `Socket`)*
- `removeAllListeners` — instance *(class: `Server`)*
- `removeListener` — instance *(class: `Socket`)*
- `removeListener` — instance *(class: `Server`)*
- `resetAndDestroy` — instance *(class: `Socket`)*
- `resume` — instance *(class: `Socket`)*
- `setDefaultAutoSelectFamily` — module
- `setDefaultAutoSelectFamilyAttemptTimeout` — module
- `setDefaultEncoding` — instance *(class: `Socket`)*
- `setEncoding` — instance *(class: `Socket`)*
- `setKeepAlive` — instance *(class: `Socket`)*
- `setNoDelay` — instance *(class: `Socket`)*
- `setTimeout` — instance *(class: `Socket`)*
- `uncork` — instance *(class: `Socket`)*
- `unref` — instance *(class: `Socket`)*
- `upgradeToTLS` — instance *(class: `Socket`)*
- `write` — instance *(class: `Socket`)*

## `node-cron`

### Methods

- `schedule` — module
- `validate` — module

## `node-fetch`

### Classes

- `Blob`
- `Headers`
- `Request`
- `Response`

### Methods

- `default` — module

## `nodemailer`

### Methods

- `createTransport` — module
- `sendMail` — instance
- `verify` — instance

## `os`

### Methods

- `arch` — module
- `availableParallelism` — module
- `cpus` — module
- `endianness` — module
- `freemem` — module
- `getPriority` — module
- `homedir` — module
- `hostname` — module
- `loadavg` — module
- `machine` — module
- `networkInterfaces` — module
- `platform` — module
- `release` — module
- `setPriority` — module
- `tmpdir` — module
- `totalmem` — module
- `type` — module
- `uptime` — module
- `userInfo` — module
- `version` — module

### Properties

- `EOL`
- `constants`
- `devNull`

## `path`

### Methods

- `basename` — module
- `dirname` — module
- `extname` — module
- `format` — module
- `isAbsolute` — module
- `join` — module
- `matchesGlob` — module
- `normalize` — module
- `parse` — module
- `relative` — module
- `resolve` — module
- `toNamespacedPath` — module

### Properties

- `delimiter`
- `posix`
- `sep`
- `win32`

## `perf_hooks`

### Classes

- `PerformanceEntry`
- `PerformanceMark`
- `PerformanceMeasure`
- `PerformanceObserver`

### Methods

- `clearMarks` — module
- `clearMeasures` — module
- `clearResourceTimings` — module
- `createHistogram` — module
- `disconnect` — instance *(class: `PerformanceObserver`)*
- `eventLoopUtilization` — module
- `getEntries` — module
- `getEntriesByName` — module
- `getEntriesByType` — module
- `mark` — module
- `markResourceTiming` — module
- `measure` — module
- `monitorEventLoopDelay` — module
- `now` — module
- `observe` — instance *(class: `PerformanceObserver`)*
- `setResourceTimingBufferSize` — module
- `takeRecords` — instance *(class: `PerformanceObserver`)*
- `timerify` — module
- `toJSON` — module

### Properties

- `constants`
- `nodeTiming`
- `performance`
- `supportedEntryTypes`
- `timeOrigin`

## `perry/ads`

### Methods

- `js_ads_banner_create` — module
- `js_ads_banner_destroy` — module
- `js_ads_interstitial_load` — module
- `js_ads_interstitial_show` — module
- `js_ads_rewarded_load` — module
- `js_ads_rewarded_show` — module

## `perry/audio`

### Methods

- `createBus` — module
- `crossfade` — module
- `destroyBus` — module
- `fadeIn` — module
- `fadeOut` — module
- `getDuration` — module
- `getPosition` — module
- `isPlaying` — module
- `loadSound` — module
- `muteBus` — module
- `onEnded` — module
- `onLoaded` — module
- `pause` — module
- `play` — module
- `resume` — module
- `resumeAll` — module
- `setMasterVolume` — module
- `setPan` — module
- `setRate` — module
- `setVolume` — module
- `soloBus` — module
- `stop` — module
- `suspend` — module
- `unload` — module

## `perry/background`

### Methods

- `cancel` — module
- `registerTask` — module
- `schedule` — module

## `perry/i18n`

### Methods

- `Currency` — module
- `FormatNumber` — module
- `FormatTime` — module
- `LongDate` — module
- `Percent` — module
- `Raw` — module
- `ShortDate` — module
- `t` — module

## `perry/media`

### Methods

- `createPlayer` — module
- `destroy` — module
- `getCurrentTime` — module
- `getDuration` — module
- `getState` — module
- `isPlaying` — module
- `onStateChange` — module
- `onTimeUpdate` — module
- `pause` — module
- `play` — module
- `seek` — module
- `setNowPlaying` — module
- `setRate` — module
- `setVolume` — module
- `stop` — module

## `perry/plugin`

### Classes

- `PluginApi`

### Methods

- `discoverPlugins` — module
- `emitEvent` — module
- `emitHook` — module
- `initPlugins` — module
- `invokeTool` — module
- `listHooks` — module
- `listPlugins` — module
- `listTools` — module
- `loadPlugin` — module
- `pluginCount` — module
- `setPluginConfig` — module
- `unloadPlugin` — module

## `perry/system`

### Methods

- `appGetLaunchUrl` — module
- `appGroupDelete` — module
- `appGroupGet` — module
- `appGroupSet` — module
- `appOnOpenUrl` — module
- `audioGetLevel` — module
- `audioGetPeak` — module
- `audioGetWaveform` — module
- `audioRegisterCallback` — module
- `audioSetOutputFilename` — module
- `audioStart` — module
- `audioStartRecording` — module
- `audioStop` — module
- `audioStopRecording` — module
- `audioUnregisterCallback` — module
- `geolocationGetCurrent` — module
- `geolocationRequestPermission` — module
- `geolocationStopWatch` — module
- `geolocationWatch` — module
- `getAppBuildNumber` — module
- `getAppIcon` — module
- `getAppVersion` — module
- `getBundleId` — module
- `getDeviceIdiom` — module
- `getDeviceModel` — module
- `getLocale` — module
- `getOSVersion` — module
- `imagePickerPick` — module
- `isDarkMode` — module
- `keychainDelete` — module
- `keychainGet` — module
- `keychainSave` — module
- `networkGetStatus` — module
- `networkOnChange` — module
- `networkStopOnChange` — module
- `notificationCancel` — module
- `notificationOnBackgroundReceive` — module
- `notificationOnReceive` — module
- `notificationOnTap` — module
- `notificationRegisterRemote` — module
- `notificationSend` — module
- `openURL` — module
- `preferencesGet` — module
- `preferencesSet` — module
- `shareText` — module
- `shareUrl` — module
- `takeScreenshot` — module

## `perry/thread`

### Methods

- `parallelFilter` — module
- `parallelMap` — module
- `spawn` — module

## `perry/tui`

### Methods

- `AnimatedSpinner` — module
- `Box` — module
- `Input` — module
- `InputAt` — module
- `List` — module
- `ProgressBar` — module
- `Select` — module
- `Spacer` — module
- `Spinner` — module
- `Table` — module
- `Tabs` — module
- `Text` — module
- `TextArea` — module
- `TextStyled` — module
- `boxSetAlignItems` — module
- `boxSetFlexBasis` — module
- `boxSetFlexBasisPct` — module
- `boxSetFlexDirection` — module
- `boxSetFlexGrow` — module
- `boxSetFlexShrink` — module
- `boxSetGap` — module
- `boxSetHeight` — module
- `boxSetHeightPct` — module
- `boxSetJustifyContent` — module
- `boxSetPadding` — module
- `boxSetPaddingEach` — module
- `boxSetWidth` — module
- `boxSetWidthPct` — module
- `columns` — instance *(class: `TuiStdout`)*
- `enter` — module
- `exit` — module
- `exit` — instance *(class: `TuiApp`)*
- `focus` — module
- `focus` — instance *(class: `FocusManager`)*
- `focusNext` — module
- `focusNext` — instance *(class: `FocusManager`)*
- `focusPrevious` — module
- `focusPrevious` — instance *(class: `FocusManager`)*
- `get` — instance *(class: `State`)*
- `get` — instance *(class: `RefBox`)*
- `render` — module
- `rows` — instance *(class: `TuiStdout`)*
- `run` — module
- `set` — instance *(class: `State`)*
- `set` — instance *(class: `RefBox`)*
- `state` — module
- `useApp` — module
- `useEffect` — module
- `useFocus` — module
- `useFocusManager` — module
- `useInput` — module
- `useMemo` — module
- `useRef` — module
- `useState` — module
- `useStateSet` — module
- `useStateTuple` — module
- `useStdout` — module
- `waitUntilExit` — module
- `waitUntilExit` — instance *(class: `TuiApp`)*
- `write` — instance *(class: `TuiStdout`)*

## `perry/ui`

### Methods

- `App` — module
- `AttributedText` — module
- `BottomNavigation` — module
- `Button` — module
- `CameraView` — module
- `Canvas` — module
- `Divider` — module
- `ForEach` — module
- `HStack` — module
- `HStackWithInsets` — module
- `Image` — module
- `ImageFile` — module
- `ImageGallery` — module
- `ImageSymbol` — module
- `LazyVStack` — module
- `NavStack` — module
- `Picker` — module
- `ProgressView` — module
- `ScrollView` — module
- `Section` — module
- `SecureField` — module
- `Slider` — module
- `Spacer` — module
- `SplitView` — module
- `State` — module
- `TabBar` — module
- `Table` — module
- `Text` — module
- `TextArea` — module
- `TextField` — module
- `Toggle` — module
- `VStack` — module
- `VStackWithInsets` — module
- `WebView` — module
- `Window` — module
- `ZStack` — module
- `addKeyboardShortcut` — module
- `alert` — module
- `alertWithButtons` — module
- `appSetMaxSize` — module
- `appSetMinSize` — module
- `appSetTimer` — module
- `attributedTextAppend` — module
- `attributedTextClear` — module
- `blur` — module
- `bottomNavAddItem` — module
- `bottomNavSetBadge` — module
- `bottomNavSetSelected` — module
- `bottomNavSetTintColor` — module
- `bottomNavSetUnselectedTintColor` — module
- `cameraFreeze` — module
- `cameraRegisterFrameCallback` — module
- `cameraSampleColor` — module
- `cameraSetOnTap` — module
- `cameraStart` — module
- `cameraStop` — module
- `cameraUnfreeze` — module
- `cameraUnregisterFrameCallback` — module
- `clipboardRead` — module
- `clipboardWrite` — module
- `currentModifiers` — module
- `embedNSView` — module
- `focus` — module
- `frameSplitAddChild` — module
- `frameSplitCreate` — module
- `imageGalleryAddImage` — module
- `imageGallerySetIndex` — module
- `isKeyDown` — module
- `lazyvstackEndRefreshing` — module
- `lazyvstackSetRefreshControl` — module
- `lazyvstackSetScrollEndCallback` — module
- `loadImage` — module
- `menuAddItem` — module
- `menuAddItemWithShortcut` — module
- `menuAddSeparator` — module
- `menuAddStandardAction` — module
- `menuAddSubmenu` — module
- `menuBarAddMenu` — module
- `menuBarAttach` — module
- `menuBarCreate` — module
- `menuClear` — module
- `menuCreate` — module
- `onActivate` — module
- `onAppKeyDown` — module
- `onAppKeyUp` — module
- `onKeyDown` — module
- `onKeyUp` — module
- `onTerminate` — module
- `openFileDialog` — module
- `openFolderDialog` — module
- `pollOpenFile` — module
- `registerGlobalHotkey` — module
- `saveFileDialog` — module
- `scrollViewSetScrollEndCallback` — module
- `scrollviewSetScrollEndCallback` — module
- `setText` — module
- `sheetCreate` — module
- `sheetDismiss` — module
- `sheetPresent` — module
- `showToast` — module
- `toolbarAddItem` — module
- `toolbarAttach` — module
- `toolbarCreate` — module
- `trayAttachMenu` — module
- `trayCreate` — module
- `trayDestroy` — module
- `trayOnClick` — module
- `traySetIcon` — module
- `traySetTooltip` — module
- `webviewCanGoBack` — module
- `webviewClearCookies` — module
- `webviewEvaluateJs` — module
- `webviewGoBack` — module
- `webviewGoForward` — module
- `webviewLoadUrl` — module
- `webviewReload` — module

## `perry/updater`

### Methods

- `clearSentinel` — module
- `compareVersions` — module
- `computeFileSha256` — module
- `getBackupPath` — module
- `getExePath` — module
- `getSentinelPath` — module
- `installUpdate` — module
- `performRollback` — module
- `readSentinel` — module
- `relaunch` — module
- `verifyHash` — module
- `verifySignature` — module
- `verifySignatureV2` — module
- `writeSentinel` — module

## `perry/widget`

### Methods

- `Widget` — module

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

### Methods

- `abort` — module
- `addListener` — module
- `availableMemory` — module
- `chdir` — module
- `constrainedMemory` — module
- `cpuUsage` — module
- `cwd` — module
- `emit` — module
- `emitWarning` — module
- `eventNames` — module
- `exit` — module
- `getActiveResourcesInfo` — module
- `getMaxListeners` — module
- `getegid` — module
- `geteuid` — module
- `getgid` — module
- `getuid` — module
- `hrtime` — module
- `kill` — module
- `listenerCount` — module
- `listeners` — module
- `loadEnvFile` — module
- `memoryUsage` — module
- `nextTick` — module
- `off` — module
- `on` — module
- `once` — module
- `prependListener` — module
- `prependOnceListener` — module
- `rawListeners` — module
- `removeAllListeners` — module
- `removeListener` — module
- `resourceUsage` — module
- `setMaxListeners` — module
- `threadCpuUsage` — module
- `umask` — module
- `uptime` — module

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

## `querystring`

### Methods

- `decode` — module
- `encode` — module
- `escape` — module
- `parse` — module
- `stringify` — module
- `unescape` — module

## `rate-limiter-flexible`

### Classes

- `RateLimiterAbstract`
- `RateLimiterMemory`

## `readline`

### Methods

- `close` — instance
- `createInterface` — module
- `on` — instance
- `question` — instance

## `redis`

### Classes

- `Redis`

### Methods

- `createClient` — module

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

## `stream`

### Classes

- `Duplex`
- `PassThrough`
- `Readable`
- `Stream`
- `Transform`
- `Writable`

### Methods

- `addAbortSignal` — module
- `addListener` — instance
- `allowHalfOpen` — instance
- `closed` — instance
- `compose` — module
- `cork` — instance
- `default` — module
- `destroy` — instance
- `destroyed` — instance
- `duplexPair` — module
- `emit` — instance
- `end` — instance
- `errored` — instance
- `eventNames` — instance
- `finished` — module
- `from` — module
- `fromWeb` — module
- `getDefaultHighWaterMark` — module
- `getMaxListeners` — instance
- `isDisturbed` — module
- `isErrored` — module
- `isPaused` — instance
- `isReadable` — module
- `isWritable` — module
- `listenerCount` — instance
- `listeners` — instance
- `off` — instance
- `on` — instance
- `once` — instance
- `pause` — instance
- `pipe` — instance
- `pipeline` — module
- `prependListener` — instance
- `prependOnceListener` — instance
- `push` — instance
- `rawListeners` — instance
- `read` — instance
- `readable` — instance
- `readableAborted` — instance
- `readableDidRead` — instance
- `readableEncoding` — instance
- `readableEnded` — instance
- `readableFlowing` — instance
- `readableHighWaterMark` — instance
- `readableLength` — instance
- `readableObjectMode` — instance
- `removeAllListeners` — instance
- `removeListener` — instance
- `resume` — instance
- `setDefaultHighWaterMark` — module
- `setEncoding` — instance
- `setMaxListeners` — instance
- `toWeb` — module
- `uncork` — instance
- `unpipe` — instance
- `writable` — instance
- `writableCorked` — instance
- `writableEnded` — instance
- `writableFinished` — instance
- `writableHighWaterMark` — instance
- `writableLength` — instance
- `writableNeedDrain` — instance
- `writableObjectMode` — instance
- `write` — instance

### Properties

- `promises`
- `promises`
- `prototype`

## `stream/promises`

### Methods

- `finished` — module
- `finished` — module
- `pipeline` — module
- `pipeline` — module

## `streams`

### Classes

- `ByteLengthQueuingStrategy`
- `CountQueuingStrategy`
- `DecompressionStream`
- `ReadableStream`
- `TextDecoder`
- `TextEncoder`
- `TransformStream`
- `WritableStream`

## `string_decoder`

### Classes

- `StringDecoder`

### Methods

- `end` — instance *(class: `StringDecoder`)*
- `write` — instance *(class: `StringDecoder`)*

### Properties

- `encoding`
- `lastChar`
- `lastNeed`
- `lastTotal`

## `tls`

### Methods

- `connect` — module

## `tty`

### Classes

- `ReadStream`
- `WriteStream`

### Methods

- `isatty` — module

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

### Methods

- `createObjectURL` — module
- `domainToASCII` — module
- `domainToUnicode` — module
- `fileURLToPath` — module
- `format` — module
- `parse` — module
- `pathToFileURL` — module
- `resolve` — module
- `revokeObjectURL` — module
- `urlToHttpOptions` — module

## `util`

### Classes

- `TextDecoder`
- `TextEncoder`

### Methods

- `callbackify` — module
- `deprecate` — module
- `format` — module
- `formatWithOptions` — module
- `inherits` — module
- `inspect` — module
- `isDeepStrictEqual` — module
- `promisify` — module
- `stripVTControlCharacters` — module

### Properties

- `types`

## `util/types`

### Methods

- `isAnyArrayBuffer` — module
- `isArrayBuffer` — module
- `isArrayBufferView` — module
- `isBooleanObject` — module
- `isBoxedPrimitive` — module
- `isDate` — module
- `isFloat64Array` — module
- `isInt32Array` — module
- `isMap` — module
- `isMapIterator` — module
- `isNumberObject` — module
- `isPromise` — module
- `isProxy` — module
- `isRegExp` — module
- `isSet` — module
- `isSetIterator` — module
- `isSharedArrayBuffer` — module
- `isStringObject` — module
- `isTypedArray` — module
- `isUint16Array` — module
- `isUint8Array` — module

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

- `getEnvironmentData` — module
- `getWorkerData` — module
- `parentPort` — module
- `postMessage` — instance
- `setEnvironmentData` — module
- `workerData` — module

### Properties

- `isMainThread`
- `resourceLimits`
- `threadId`

## `ws`

### Classes

- `Client`
- `WebSocket`
- `WebSocketServer`

### Methods

- `Server` — module
- `WebSocket` — module
- `addListener` — instance *(class: `Client`)*
- `close` — instance
- `close` — instance *(class: `Client`)*
- `closeClient` — module
- `handleUpgrade` — instance
- `on` — instance
- `on` — instance *(class: `Client`)*
- `send` — instance
- `send` — instance *(class: `Client`)*
- `sendToClient` — module

## `zlib`

### Classes

- `BrotliCompress`
- `BrotliCompress`
- `BrotliDecompress`
- `BrotliDecompress`
- `Deflate`
- `Deflate`
- `DeflateRaw`
- `DeflateRaw`
- `Gunzip`
- `Gunzip`
- `Gzip`
- `Gzip`
- `Inflate`
- `Inflate`
- `InflateRaw`
- `InflateRaw`
- `Unzip`
- `Unzip`
- `ZstdCompress`
- `ZstdDecompress`

### Methods

- `brotliCompress` — module
- `brotliCompressSync` — module
- `brotliDecompress` — module
- `brotliDecompressSync` — module
- `crc32` — module
- `createBrotliCompress` — module
- `createBrotliDecompress` — module
- `createDeflate` — module
- `createDeflateRaw` — module
- `createGunzip` — module
- `createGzip` — module
- `createInflate` — module
- `createInflateRaw` — module
- `createUnzip` — module
- `createZstdCompress` — module
- `createZstdDecompress` — module
- `deflate` — module
- `deflateRaw` — module
- `deflateRawSync` — module
- `deflateSync` — module
- `gunzip` — module
- `gunzipSync` — module
- `gzip` — module
- `gzipSync` — module
- `inflate` — module
- `inflateRaw` — module
- `inflateRawSync` — module
- `inflateSync` — module
- `unzip` — module
- `unzipSync` — module

### Properties

- `constants`
