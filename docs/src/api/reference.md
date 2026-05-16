# Supported API Reference

This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.

Total: 848 entries across 71 modules.

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
- [`rate-limiter-flexible`](#rate-limiter-flexible)
- [`readline`](#readline)
- [`redis`](#redis)
- [`sharp`](#sharp)
- [`slugify`](#slugify)
- [`stream`](#stream)
- [`streams`](#streams)
- [`string_decoder`](#string-decoder)
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
- `default` — module
- `exec` — instance
- `get` — instance
- `prepare` — instance
- `raw` — instance
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

### Properties

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
- `IncomingMessage`
- `Server`
- `Server`
- `ServerResponse`
- `ServerResponse`

### Methods

- `__get_aborted` — instance *(class: `IncomingMessage`)*
- `__get_complete` — instance *(class: `IncomingMessage`)*
- `__get_destroyed` — instance *(class: `IncomingMessage`)*
- `__get_headersSent` — instance *(class: `ServerResponse`)*
- `__get_httpVersion` — instance *(class: `IncomingMessage`)*
- `__get_method` — instance *(class: `IncomingMessage`)*
- `__get_statusCode` — instance *(class: `ServerResponse`)*
- `__get_url` — instance *(class: `IncomingMessage`)*
- `__get_writableEnded` — instance *(class: `ServerResponse`)*
- `__get_writableFinished` — instance *(class: `ServerResponse`)*
- `__set_statusCode` — instance *(class: `ServerResponse`)*
- `__set_statusMessage` — instance *(class: `ServerResponse`)*
- `addListener` — instance *(class: `HttpServer`)*
- `addListener` — instance *(class: `IncomingMessage`)*
- `addListener` — instance *(class: `ServerResponse`)*
- `close` — instance *(class: `HttpServer`)*
- `closeAllConnections` — instance *(class: `HttpServer`)*
- `closeIdleConnections` — instance *(class: `HttpServer`)*
- `createServer` — module
- `createServer` — module
- `destroy` — instance *(class: `IncomingMessage`)*
- `end` — instance *(class: `ServerResponse`)*
- `flushHeaders` — instance *(class: `ServerResponse`)*
- `get` — module
- `getHeader` — instance *(class: `ServerResponse`)*
- `getStatus` — instance *(class: `ServerResponse`)*
- `hasHeader` — instance *(class: `ServerResponse`)*
- `httpVersion` — instance *(class: `IncomingMessage`)*
- `listen` — instance *(class: `HttpServer`)*
- `method` — instance *(class: `IncomingMessage`)*
- `on` — instance *(class: `HttpServer`)*
- `on` — instance *(class: `IncomingMessage`)*
- `on` — instance *(class: `ServerResponse`)*
- `pause` — instance *(class: `IncomingMessage`)*
- `read` — instance *(class: `IncomingMessage`)*
- `removeHeader` — instance *(class: `ServerResponse`)*
- `request` — module
- `resume` — instance *(class: `IncomingMessage`)*
- `setHeader` — instance *(class: `ServerResponse`)*
- `setStatus` — instance *(class: `ServerResponse`)*
- `setTimeout` — instance *(class: `ClientRequest`)*
- `url` — instance *(class: `IncomingMessage`)*
- `write` — instance *(class: `ServerResponse`)*
- `writeContinue` — instance *(class: `ServerResponse`)*
- `writeHead` — instance *(class: `ServerResponse`)*
- `writeProcessing` — instance *(class: `ServerResponse`)*

## `http2`

### Classes

- `Http2SecureServer`
- `Http2ServerRequest`
- `Http2ServerResponse`

### Methods

- `close` — instance *(class: `Http2SecureServer`)*
- `createSecureServer` — module
- `listen` — instance *(class: `Http2SecureServer`)*
- `on` — instance *(class: `Http2SecureServer`)*

## `https`

### Classes

- `ClientRequest`
- `IncomingMessage`
- `Server`
- `Server`
- `ServerResponse`

### Methods

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
- `getDefaultAutoSelectFamily` — module
- `getDefaultAutoSelectFamilyAttemptTimeout` — module
- `isIP` — module
- `isIPv4` — module
- `isIPv6` — module
- `on` — instance *(class: `Socket`)*
- `setDefaultAutoSelectFamily` — module
- `setDefaultAutoSelectFamilyAttemptTimeout` — module
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
- `constants`

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
- `appOnOpenUrl` — module
- `audioGetLevel` — module
- `audioGetPeak` — module
- `audioGetWaveform` — module
- `audioSetOutputFilename` — module
- `audioStart` — module
- `audioStartRecording` — module
- `audioStop` — module
- `audioStopRecording` — module
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
- `bottomNavAddItem` — module
- `bottomNavSetBadge` — module
- `bottomNavSetSelected` — module
- `bottomNavSetTintColor` — module
- `bottomNavSetUnselectedTintColor` — module
- `clipboardRead` — module
- `clipboardWrite` — module
- `embedNSView` — module
- `frameSplitAddChild` — module
- `frameSplitCreate` — module
- `imageGalleryAddImage` — module
- `imageGallerySetIndex` — module
- `lazyvstackEndRefreshing` — module
- `lazyvstackSetRefreshControl` — module
- `lazyvstackSetScrollEndCallback` — module
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
- `from` — module
- `pipeline` — module

## `streams`

### Classes

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

### Properties

- `types`

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
- `on` — instance
- `on` — instance *(class: `Client`)*
- `send` — instance
- `send` — instance *(class: `Client`)*
- `sendToClient` — module

## `zlib`

### Methods

- `deflateSync` — module
- `gunzip` — module
- `gunzipSync` — module
- `gzip` — module
- `gzipSync` — module
- `inflateSync` — module

