# Supported API Reference

This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.

**Generated for Perry v0.5.600.**

Total: 673 entries across 68 modules.

## Modules

- [`argon2`](#argon2)
- [`async_hooks`](#async-hooks)
- [`axios`](#axios)
- [`bcrypt`](#bcrypt)
- [`better-sqlite3`](#better-sqlite3)
- [`bignumber.js`](#bignumber-js)
- [`buffer`](#buffer)
- [`cheerio`](#cheerio)
- [`child_process`](#child-process)
- [`commander`](#commander)
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
- [`rate-limiter-flexible`](#rate-limiter-flexible)
- [`readline`](#readline)
- [`redis`](#redis)
- [`sharp`](#sharp)
- [`slugify`](#slugify)
- [`stream`](#stream)
- [`streams`](#streams)
- [`tls`](#tls)
- [`tty`](#tty)
- [`tursodb`](#tursodb)
- [`url`](#url)
- [`util`](#util)
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
- `default` — module
- `exec` — instance
- `get` — instance
- `prepare` — instance
- `run` — instance

## `bignumber.js`

### Classes

- `BigNumber`

## `buffer`

### Classes

- `Buffer`

### Methods

- `alloc` — module
- `allocUnsafe` — module
- `byteLength` — module
- `concat` — module
- `from` — module
- `isBuffer` — module

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

### Methods

- `exec` — module
- `execFile` — module
- `execFileSync` — module
- `execSync` — module
- `fork` — module
- `spawn` — module
- `spawnSync` — module

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

- `accessSync` — module
- `appendFile` — module
- `appendFileSync` — module
- `chmodSync` — module
- `copyFileSync` — module
- `createReadStream` — module
- `createWriteStream` — module
- `existsSync` — module
- `lstatSync` — module
- `mkdir` — module
- `mkdirSync` — module
- `mkdtempSync` — module
- `readFile` — module
- `readFileSync` — module
- `readdir` — module
- `readdirSync` — module
- `realpathSync` — module
- `renameSync` — module
- `rm` — module
- `rmSync` — module
- `rmdirSync` — module
- `stat` — module
- `statSync` — module
- `unlink` — module
- `unlinkSync` — module
- `unwatchFile` — module
- `watchFile` — module
- `writeFile` — module
- `writeFileSync` — module

### Properties

- `constants`
- `promises`

## `http`

### Classes

- `ClientRequest`
- `IncomingMessage`
- `Server`
- `ServerResponse`

### Methods

- `createServer` — module
- `get` — module
- `request` — module

## `https`

### Classes

- `ClientRequest`
- `IncomingMessage`
- `Server`
- `ServerResponse`

### Methods

- `createServer` — module
- `get` — module
- `request` — module

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

- `audioGetLevel` — module
- `audioGetPeak` — module
- `audioGetWaveform` — module
- `audioSetOutputFilename` — module
- `audioStart` — module
- `audioStartRecording` — module
- `audioStop` — module
- `audioStopRecording` — module
- `getAppIcon` — module
- `getDeviceIdiom` — module
- `getDeviceModel` — module
- `getLocale` — module
- `isDarkMode` — module
- `keychainDelete` — module
- `keychainGet` — module
- `keychainSave` — module
- `notificationCancel` — module
- `notificationOnBackgroundReceive` — module
- `notificationOnReceive` — module
- `notificationOnTap` — module
- `notificationRegisterRemote` — module
- `notificationSend` — module
- `openURL` — module
- `preferencesGet` — module
- `preferencesSet` — module

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

## `perry/ui`

### Methods

- `App` — module
- `Button` — module
- `CameraView` — module
- `Canvas` — module
- `Divider` — module
- `ForEach` — module
- `HStack` — module
- `HStackWithInsets` — module
- `ImageFile` — module
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
- `Window` — module
- `ZStack` — module
- `addKeyboardShortcut` — module
- `alert` — module
- `alertWithButtons` — module
- `appSetMaxSize` — module
- `appSetMinSize` — module
- `appSetTimer` — module
- `clipboardRead` — module
- `clipboardWrite` — module
- `embedNSView` — module
- `frameSplitAddChild` — module
- `frameSplitCreate` — module
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
- `onTerminate` — module
- `openFileDialog` — module
- `openFolderDialog` — module
- `pollOpenFile` — module
- `registerGlobalHotkey` — module
- `saveFileDialog` — module
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
- `Transform`
- `Writable`

### Methods

- `finished` — module
- `pipeline` — module

## `streams`

### Classes

- `DecompressionStream`
- `ReadableStream`
- `TextDecoder`
- `TextEncoder`
- `TransformStream`
- `WritableStream`

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

- `fileURLToPath` — module
- `format` — module
- `parse` — module
- `pathToFileURL` — module

## `util`

### Classes

- `TextDecoder`
- `TextEncoder`

### Methods

- `callbackify` — module
- `deprecate` — module
- `format` — module
- `inherits` — module
- `inspect` — module
- `isDeepStrictEqual` — module
- `promisify` — module

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

