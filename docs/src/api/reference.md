# Supported API Reference

This page is auto-generated from Perry's compile-time API manifest (`perry-api-manifest::API_MANIFEST`). It is the source of truth for what `perry compile` accepts; references to symbols not listed here produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are flagged ⚠ — they link cleanly but no-op at runtime on the chosen target.

Total: 2826 entries across 116 modules.

## Modules

- [`@perryts/pdf`](#-perryts-pdf)
- [`__disposable__`](#--disposable--)
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
- [`constants`](#constants)
- [`cron`](#cron)
- [`crypto`](#crypto)
- [`date-fns`](#date-fns)
- [`dayjs`](#dayjs)
- [`decimal.js`](#decimal-js)
- [`dgram`](#dgram)
- [`diagnostics_channel`](#diagnostics-channel)
- [`dns`](#dns)
- [`dns/promises`](#dns-promises)
- [`domain`](#domain)
- [`dotenv`](#dotenv)
- [`ethers`](#ethers)
- [`events`](#events)
- [`exponential-backoff`](#exponential-backoff)
- [`fastify`](#fastify)
- [`fetch`](#fetch)
- [`fs`](#fs)
- [`fs/promises`](#fs-promises)
- [`http`](#http)
- [`http2`](#http2)
- [`https`](#https)
- [`inspector`](#inspector)
- [`inspector/promises`](#inspector-promises)
- [`ioredis`](#ioredis)
- [`iroh`](#iroh)
- [`jsonwebtoken`](#jsonwebtoken)
- [`lodash`](#lodash)
- [`lru-cache`](#lru-cache)
- [`module`](#module)
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
- [`path/posix`](#path-posix)
- [`path/win32`](#path-win32)
- [`perf_hooks`](#perf-hooks)
- [`perry`](#perry)
- [`perry/ads`](#perry-ads)
- [`perry/audio`](#perry-audio)
- [`perry/background`](#perry-background)
- [`perry/compose`](#perry-compose)
- [`perry/container`](#perry-container)
- [`perry/container-compose`](#perry-container-compose)
- [`perry/i18n`](#perry-i18n)
- [`perry/media`](#perry-media)
- [`perry/plugin`](#perry-plugin)
- [`perry/system`](#perry-system)
- [`perry/thread`](#perry-thread)
- [`perry/tui`](#perry-tui)
- [`perry/ui`](#perry-ui)
- [`perry/updater`](#perry-updater)
- [`perry/widget`](#perry-widget)
- [`perry/workloads`](#perry-workloads)
- [`perry/yoga`](#perry-yoga)
- [`pg`](#pg)
- [`process`](#process)
- [`punycode`](#punycode)
- [`querystring`](#querystring)
- [`rate-limiter-flexible`](#rate-limiter-flexible)
- [`readline`](#readline)
- [`readline/promises`](#readline-promises)
- [`redis`](#redis)
- [`repl`](#repl)
- [`sea`](#sea)
- [`sharp`](#sharp)
- [`slugify`](#slugify)
- [`sqlite`](#sqlite)
- [`stream`](#stream)
- [`stream/consumers`](#stream-consumers)
- [`stream/promises`](#stream-promises)
- [`stream/web`](#stream-web)
- [`streams`](#streams)
- [`string_decoder`](#string-decoder)
- [`sys`](#sys)
- [`test`](#test)
- [`test/reporters`](#test-reporters)
- [`timers`](#timers)
- [`timers/promises`](#timers-promises)
- [`tls`](#tls)
- [`tty`](#tty)
- [`tursodb`](#tursodb)
- [`url`](#url)
- [`util`](#util)
- [`util/types`](#util-types)
- [`uuid`](#uuid)
- [`v8`](#v8)
- [`validator`](#validator)
- [`vm`](#vm)
- [`wasi`](#wasi)
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

## `__disposable__`

### Methods

- `adopt` — instance
- `defer` — instance
- `dispose` — instance
- `disposeAsync` — instance
- `disposed` — instance
- `move` — instance
- `use` — instance

## `argon2`

### Methods

- `hash` — module
- `verify` — module

## `assert`

### Classes

- `Assert`
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
- `partialDeepStrictEqual` — module
- `rejects` — module
- `strict` — module
- `strictEqual` — module
- `throws` — module

### Properties

- `strict`

## `assert/strict`

### Classes

- `Assert`
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
- `partialDeepStrictEqual` — module
- `rejects` — module
- `strict` — module
- `strictEqual` — module
- `throws` — module

### Properties

- `strict`

## `async_hooks`

### Classes

- `AsyncLocalStorage`
- `AsyncResource`

### Methods

- `asyncId` — instance *(class: `AsyncResource`)*
- `bind` — module *(class: `AsyncLocalStorage`)*
- `bind` — module *(class: `AsyncResource`)*
- `bind` — instance *(class: `AsyncResource`)*
- `createHook` — module
- `disable` — instance
- `emitDestroy` — instance *(class: `AsyncResource`)*
- `enable` — instance *(class: `AsyncHook`)*
- `enterWith` — instance
- `executionAsyncId` — module
- `executionAsyncResource` — module
- `exit` — instance
- `getStore` — instance
- `run` — instance
- `runInAsyncScope` — instance *(class: `AsyncResource`)*
- `snapshot` — module *(class: `AsyncLocalStorage`)*
- `triggerAsyncId` — module
- `triggerAsyncId` — instance *(class: `AsyncResource`)*

### Properties

- `asyncWrapProviders`
- `default`

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

- `atob` — module
- `btoa` — module
- `isAscii` — module
- `isUtf8` — module
- `resolveObjectURL` — module
- `transcode` — module

### Properties

- `INSPECT_MAX_BYTES`
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

- `_forkChild` — module
- `exec` — module
- `execFile` — module
- `execFileSync` — module
- `execSync` — module
- `fork` — module
- `spawn` — module
- `spawnSync` — module

### Properties

- `default`

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
- `default`
- `isMaster`
- `isPrimary`
- `isWorker`
- `schedulingPolicy`
- `settings`
- `workers`

## `commander`

### Methods

- `action` — instance
- `args` — instance
- `argument` — instance
- `command` — instance
- `description` — instance
- `name` — instance
- `option` — instance
- `opts` — instance
- `parse` — instance
- `requiredOption` — instance
- `version` — instance

### Properties

- `args`

## `console`

### Classes

- `Console`

### Methods

- `assert` — module
- `clear` — module
- `context` — module
- `count` — module
- `countReset` — module
- `createTask` — module
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

## `constants`

### Properties

- `COPYFILE_EXCL`
- `COPYFILE_FICLONE`
- `COPYFILE_FICLONE_FORCE`
- `DH_CHECK_P_NOT_PRIME`
- `DH_CHECK_P_NOT_SAFE_PRIME`
- `DH_NOT_SUITABLE_GENERATOR`
- `DH_UNABLE_TO_CHECK_GENERATOR`
- `E2BIG`
- `EACCES`
- `EADDRINUSE`
- `EADDRNOTAVAIL`
- `EAFNOSUPPORT`
- `EAGAIN`
- `EALREADY`
- `EBADF`
- `EBADMSG`
- `EBUSY`
- `ECANCELED`
- `ECHILD`
- `ECONNABORTED`
- `ECONNREFUSED`
- `ECONNRESET`
- `EDEADLK`
- `EDESTADDRREQ`
- `EDOM`
- `EDQUOT`
- `EEXIST`
- `EFAULT`
- `EFBIG`
- `EHOSTUNREACH`
- `EIDRM`
- `EILSEQ`
- `EINPROGRESS`
- `EINTR`
- `EINVAL`
- `EIO`
- `EISCONN`
- `EISDIR`
- `ELOOP`
- `EMFILE`
- `EMLINK`
- `EMSGSIZE`
- `EMULTIHOP`
- `ENAMETOOLONG`
- `ENETDOWN`
- `ENETRESET`
- `ENETUNREACH`
- `ENFILE`
- `ENGINE_METHOD_ALL`
- `ENGINE_METHOD_CIPHERS`
- `ENGINE_METHOD_DH`
- `ENGINE_METHOD_DIGESTS`
- `ENGINE_METHOD_DSA`
- `ENGINE_METHOD_EC`
- `ENGINE_METHOD_NONE`
- `ENGINE_METHOD_PKEY_ASN1_METHS`
- `ENGINE_METHOD_PKEY_METHS`
- `ENGINE_METHOD_RAND`
- `ENGINE_METHOD_RSA`
- `ENOBUFS`
- `ENODATA`
- `ENODEV`
- `ENOENT`
- `ENOEXEC`
- `ENOLCK`
- `ENOLINK`
- `ENOMEM`
- `ENOMSG`
- `ENOPROTOOPT`
- `ENOSPC`
- `ENOSR`
- `ENOSTR`
- `ENOSYS`
- `ENOTCONN`
- `ENOTDIR`
- `ENOTEMPTY`
- `ENOTSOCK`
- `ENOTSUP`
- `ENOTTY`
- `ENXIO`
- `EOPNOTSUPP`
- `EOVERFLOW`
- `EPERM`
- `EPIPE`
- `EPROTO`
- `EPROTONOSUPPORT`
- `EPROTOTYPE`
- `ERANGE`
- `EROFS`
- `ESPIPE`
- `ESRCH`
- `ESTALE`
- `ETIME`
- `ETIMEDOUT`
- `ETXTBSY`
- `EWOULDBLOCK`
- `EXDEV`
- `F_OK`
- `OPENSSL_VERSION_NUMBER`
- `O_APPEND`
- `O_CREAT`
- `O_DIRECT`
- `O_DIRECTORY`
- `O_DSYNC`
- `O_EXCL`
- `O_NOATIME`
- `O_NOCTTY`
- `O_NOFOLLOW`
- `O_NONBLOCK`
- `O_RDONLY`
- `O_RDWR`
- `O_SYMLINK`
- `O_SYNC`
- `O_TRUNC`
- `O_WRONLY`
- `POINT_CONVERSION_COMPRESSED`
- `POINT_CONVERSION_HYBRID`
- `POINT_CONVERSION_UNCOMPRESSED`
- `PRIORITY_ABOVE_NORMAL`
- `PRIORITY_BELOW_NORMAL`
- `PRIORITY_HIGH`
- `PRIORITY_HIGHEST`
- `PRIORITY_LOW`
- `PRIORITY_NORMAL`
- `RSA_NO_PADDING`
- `RSA_PKCS1_OAEP_PADDING`
- `RSA_PKCS1_PADDING`
- `RSA_PKCS1_PSS_PADDING`
- `RSA_PSS_SALTLEN_AUTO`
- `RSA_PSS_SALTLEN_DIGEST`
- `RSA_PSS_SALTLEN_MAX_SIGN`
- `RSA_X931_PADDING`
- `RTLD_DEEPBIND`
- `RTLD_GLOBAL`
- `RTLD_LAZY`
- `RTLD_LOCAL`
- `RTLD_NOW`
- `R_OK`
- `SIGABRT`
- `SIGALRM`
- `SIGBUS`
- `SIGCHLD`
- `SIGCONT`
- `SIGFPE`
- `SIGHUP`
- `SIGILL`
- `SIGINFO`
- `SIGINT`
- `SIGIO`
- `SIGIOT`
- `SIGKILL`
- `SIGPIPE`
- `SIGPOLL`
- `SIGPROF`
- `SIGPWR`
- `SIGQUIT`
- `SIGSEGV`
- `SIGSTKFLT`
- `SIGSTOP`
- `SIGSYS`
- `SIGTERM`
- `SIGTRAP`
- `SIGTSTP`
- `SIGTTIN`
- `SIGTTOU`
- `SIGURG`
- `SIGUSR1`
- `SIGUSR2`
- `SIGVTALRM`
- `SIGWINCH`
- `SIGXCPU`
- `SIGXFSZ`
- `SSL_OP_ALL`
- `SSL_OP_ALLOW_NO_DHE_KEX`
- `SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION`
- `SSL_OP_CIPHER_SERVER_PREFERENCE`
- `SSL_OP_CISCO_ANYCONNECT`
- `SSL_OP_COOKIE_EXCHANGE`
- `SSL_OP_CRYPTOPRO_TLSEXT_BUG`
- `SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS`
- `SSL_OP_LEGACY_SERVER_CONNECT`
- `SSL_OP_NO_COMPRESSION`
- `SSL_OP_NO_ENCRYPT_THEN_MAC`
- `SSL_OP_NO_QUERY_MTU`
- `SSL_OP_NO_RENEGOTIATION`
- `SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION`
- `SSL_OP_NO_SSLv2`
- `SSL_OP_NO_SSLv3`
- `SSL_OP_NO_TICKET`
- `SSL_OP_NO_TLSv1`
- `SSL_OP_NO_TLSv1_1`
- `SSL_OP_NO_TLSv1_2`
- `SSL_OP_NO_TLSv1_3`
- `SSL_OP_PRIORITIZE_CHACHA`
- `SSL_OP_TLS_ROLLBACK_BUG`
- `S_IFBLK`
- `S_IFCHR`
- `S_IFDIR`
- `S_IFIFO`
- `S_IFLNK`
- `S_IFMT`
- `S_IFREG`
- `S_IFSOCK`
- `S_IRGRP`
- `S_IROTH`
- `S_IRUSR`
- `S_IRWXG`
- `S_IRWXO`
- `S_IRWXU`
- `S_IWGRP`
- `S_IWOTH`
- `S_IWUSR`
- `S_IXGRP`
- `S_IXOTH`
- `S_IXUSR`
- `TLS1_1_VERSION`
- `TLS1_2_VERSION`
- `TLS1_3_VERSION`
- `TLS1_VERSION`
- `UV_DIRENT_BLOCK`
- `UV_DIRENT_CHAR`
- `UV_DIRENT_DIR`
- `UV_DIRENT_FIFO`
- `UV_DIRENT_FILE`
- `UV_DIRENT_LINK`
- `UV_DIRENT_SOCKET`
- `UV_DIRENT_UNKNOWN`
- `UV_FS_COPYFILE_EXCL`
- `UV_FS_COPYFILE_FICLONE`
- `UV_FS_COPYFILE_FICLONE_FORCE`
- `UV_FS_O_FILEMAP`
- `UV_FS_SYMLINK_DIR`
- `UV_FS_SYMLINK_JUNCTION`
- `W_OK`
- `X_OK`
- `default`
- `defaultCoreCipherList`

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

- `Cipheriv`
- `Decipheriv`
- `DiffieHellman`
- `DiffieHellmanGroup`
- `ECDH`
- `KeyObject`
- `X509Certificate`

### Methods

- `Hash` — module
- `Hmac` — module
- `Sign` — module
- `Verify` — module
- `argon2` — module
- `argon2Sync` — module
- `checkPrime` — module
- `checkPrimeSync` — module
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
- `decapsulate` — module
- `diffieHellman` — module
- `encapsulate` — module
- `generateKey` — module
- `generateKeyPair` — module
- `generateKeyPairSync` — module
- `generateKeyPairSync` — module
- `generateKeySync` — module
- `generatePrime` — module
- `generatePrimeSync` — module
- `getCipherInfo` — module
- `getCiphers` — module
- `getCurves` — module
- `getDiffieHellman` — module
- `getFips` — module
- `getHashes` — module
- `getRandomValues` — module
- `hash` — module
- `hkdf` — module
- `hkdfSync` — module
- `pbkdf2` — module
- `pbkdf2Sync` — module
- `privateDecrypt` — module
- `privateEncrypt` — module
- `publicDecrypt` — module
- `publicEncrypt` — module
- `randomBytes` — module
- `randomFill` — module
- `randomFillSync` — module
- `randomInt` — module
- `randomInt` — module
- `randomUUID` — module
- `scrypt` — module
- `scryptSync` — module
- `secureHeapUsed` — module
- `setFips` — module
- `sign` — module
- `timingSafeEqual` — module
- `verify` — module

### Properties

- `Certificate`
- `constants`
- `subtle`
- `webcrypto`

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

## `dgram`

### Classes

- `Socket`

### Methods

- `Socket` — module
- `addListener` — instance *(class: `Socket`)*
- `addMembership` — instance *(class: `Socket`)*
- `addSourceSpecificMembership` — instance *(class: `Socket`)*
- `address` — instance *(class: `Socket`)*
- `bind` — instance *(class: `Socket`)*
- `close` — instance *(class: `Socket`)*
- `connect` — instance *(class: `Socket`)*
- `createSocket` — module
- `disconnect` — instance *(class: `Socket`)*
- `dropMembership` — instance *(class: `Socket`)*
- `dropSourceSpecificMembership` — instance *(class: `Socket`)*
- `emit` — instance *(class: `Socket`)*
- `eventNames` — instance *(class: `Socket`)*
- `getRecvBufferSize` — instance *(class: `Socket`)*
- `getSendBufferSize` — instance *(class: `Socket`)*
- `getSendQueueCount` — instance *(class: `Socket`)*
- `getSendQueueSize` — instance *(class: `Socket`)*
- `listenerCount` — instance *(class: `Socket`)*
- `off` — instance *(class: `Socket`)*
- `on` — instance *(class: `Socket`)*
- `once` — instance *(class: `Socket`)*
- `ref` — instance *(class: `Socket`)*
- `remoteAddress` — instance *(class: `Socket`)*
- `removeListener` — instance *(class: `Socket`)*
- `send` — instance *(class: `Socket`)*
- `setBroadcast` — instance *(class: `Socket`)*
- `setMulticastInterface` — instance *(class: `Socket`)*
- `setMulticastLoopback` — instance *(class: `Socket`)*
- `setMulticastTTL` — instance *(class: `Socket`)*
- `setRecvBufferSize` — instance *(class: `Socket`)*
- `setSendBufferSize` — instance *(class: `Socket`)*
- `setTTL` — instance *(class: `Socket`)*
- `unref` — instance *(class: `Socket`)*

### Properties

- `default`

## `diagnostics_channel`

### Classes

- `BoundedChannel`
- `Channel`

### Methods

- `boundedChannel` — module
- `channel` — module
- `hasSubscribers` — module
- `subscribe` — module
- `tracingChannel` — module
- `unsubscribe` — module

### Properties

- `default`

## `dns`

### Classes

- `Resolver`

### Methods

- `Resolver` — module
- `cancel` — instance *(class: `Resolver`)*
- `getDefaultResultOrder` — module
- `getServers` — module
- `getServers` — instance *(class: `Resolver`)*
- `lookup` — module
- `lookupService` — module
- `resolve` — module
- `resolve` — instance *(class: `Resolver`)*
- `resolve4` — module
- `resolve4` — instance *(class: `Resolver`)*
- `resolve6` — module
- `resolve6` — instance *(class: `Resolver`)*
- `resolveAny` — module
- `resolveAny` — instance *(class: `Resolver`)*
- `resolveCaa` — module
- `resolveCaa` — instance *(class: `Resolver`)*
- `resolveCname` — module
- `resolveCname` — instance *(class: `Resolver`)*
- `resolveMx` — module
- `resolveMx` — instance *(class: `Resolver`)*
- `resolveNaptr` — module
- `resolveNaptr` — instance *(class: `Resolver`)*
- `resolveNs` — module
- `resolveNs` — instance *(class: `Resolver`)*
- `resolvePtr` — module
- `resolvePtr` — instance *(class: `Resolver`)*
- `resolveSoa` — module
- `resolveSoa` — instance *(class: `Resolver`)*
- `resolveSrv` — module
- `resolveSrv` — instance *(class: `Resolver`)*
- `resolveTlsa` — module
- `resolveTlsa` — instance *(class: `Resolver`)*
- `resolveTxt` — module
- `resolveTxt` — instance *(class: `Resolver`)*
- `reverse` — module
- `reverse` — instance *(class: `Resolver`)*
- `setDefaultResultOrder` — module
- `setLocalAddress` — instance *(class: `Resolver`)*
- `setServers` — module
- `setServers` — instance *(class: `Resolver`)*

### Properties

- `ADDRCONFIG`
- `ADDRCONFIG`
- `ADDRGETNETWORKPARAMS`
- `ADDRGETNETWORKPARAMS`
- `ALL`
- `ALL`
- `BADFAMILY`
- `BADFAMILY`
- `BADFLAGS`
- `BADFLAGS`
- `BADHINTS`
- `BADHINTS`
- `BADNAME`
- `BADNAME`
- `BADQUERY`
- `BADQUERY`
- `BADRESP`
- `BADRESP`
- `BADSTR`
- `BADSTR`
- `CANCELLED`
- `CANCELLED`
- `CONNREFUSED`
- `CONNREFUSED`
- `DESTRUCTION`
- `DESTRUCTION`
- `EOF`
- `EOF`
- `FILE`
- `FILE`
- `FORMERR`
- `FORMERR`
- `LOADIPHLPAPI`
- `LOADIPHLPAPI`
- `NODATA`
- `NODATA`
- `NOMEM`
- `NOMEM`
- `NONAME`
- `NONAME`
- `NOTFOUND`
- `NOTFOUND`
- `NOTIMP`
- `NOTIMP`
- `NOTINITIALIZED`
- `NOTINITIALIZED`
- `REFUSED`
- `REFUSED`
- `SERVFAIL`
- `SERVFAIL`
- `TIMEOUT`
- `TIMEOUT`
- `V4MAPPED`
- `V4MAPPED`
- `default`
- `promises`

## `dns/promises`

### Classes

- `Resolver`

### Methods

- `Resolver` — module
- `cancel` — instance *(class: `Resolver`)*
- `getDefaultResultOrder` — module
- `getServers` — module
- `getServers` — instance *(class: `Resolver`)*
- `lookup` — module
- `lookupService` — module
- `resolve` — module
- `resolve` — instance *(class: `Resolver`)*
- `resolve4` — module
- `resolve4` — instance *(class: `Resolver`)*
- `resolve6` — module
- `resolve6` — instance *(class: `Resolver`)*
- `resolveAny` — module
- `resolveAny` — instance *(class: `Resolver`)*
- `resolveCaa` — module
- `resolveCaa` — instance *(class: `Resolver`)*
- `resolveCname` — module
- `resolveCname` — instance *(class: `Resolver`)*
- `resolveMx` — module
- `resolveMx` — instance *(class: `Resolver`)*
- `resolveNaptr` — module
- `resolveNaptr` — instance *(class: `Resolver`)*
- `resolveNs` — module
- `resolveNs` — instance *(class: `Resolver`)*
- `resolvePtr` — module
- `resolvePtr` — instance *(class: `Resolver`)*
- `resolveSoa` — module
- `resolveSoa` — instance *(class: `Resolver`)*
- `resolveSrv` — module
- `resolveSrv` — instance *(class: `Resolver`)*
- `resolveTlsa` — module
- `resolveTlsa` — instance *(class: `Resolver`)*
- `resolveTxt` — module
- `resolveTxt` — instance *(class: `Resolver`)*
- `reverse` — module
- `reverse` — instance *(class: `Resolver`)*
- `setDefaultResultOrder` — module
- `setLocalAddress` — instance *(class: `Resolver`)*
- `setServers` — module
- `setServers` — instance *(class: `Resolver`)*

### Properties

- `ADDRGETNETWORKPARAMS`
- `BADFAMILY`
- `BADFLAGS`
- `BADHINTS`
- `BADNAME`
- `BADQUERY`
- `BADRESP`
- `BADSTR`
- `CANCELLED`
- `CONNREFUSED`
- `DESTRUCTION`
- `EOF`
- `FILE`
- `FORMERR`
- `LOADIPHLPAPI`
- `NODATA`
- `NOMEM`
- `NONAME`
- `NOTFOUND`
- `NOTIMP`
- `NOTINITIALIZED`
- `REFUSED`
- `SERVFAIL`
- `TIMEOUT`
- `default`

## `domain`

### Classes

- `Domain`

### Methods

- `Domain` — module
- `add` — instance
- `addListener` — instance
- `bind` — instance
- `create` — module
- `createDomain` — module
- `emit` — instance
- `enter` — instance
- `exit` — instance
- `intercept` — instance
- `on` — instance
- `remove` — instance
- `run` — instance

### Properties

- `_stack`
- `active`
- `members`

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
- `EventEmitterAsyncResource`

### Methods

- `EventEmitter` — module
- `EventEmitterAsyncResource` — module
- `addAbortListener` — module
- `addListener` — instance
- `asyncId` — instance *(class: `EventEmitterAsyncResource`)*
- `asyncResource` — instance *(class: `EventEmitterAsyncResource`)*
- `domain` — instance
- `emit` — instance
- `emitDestroy` — instance *(class: `EventEmitterAsyncResource`)*
- `eventNames` — instance
- `getEventListeners` — module
- `getMaxListeners` — instance
- `getMaxListeners` — module
- `init` — module
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
- `triggerAsyncId` — instance *(class: `EventEmitterAsyncResource`)*

### Properties

- `captureRejectionSymbol`
- `captureRejections`
- `default`
- `defaultMaxListeners`
- `errorMonitor`
- `usingDomains`

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
- `FormData`
- `Headers`
- `Request`
- `Response`

### Methods

- `default` — module

## `fs`

### Classes

- `Dir`
- `Dirent`
- `FileReadStream`
- `FileWriteStream`
- `ReadStream`
- `Stats`
- `Utf8Stream`
- `WriteStream`

### Methods

- `_toUnixTimestamp` — module
- `_toUnixTimestamp` — module
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
- `mkdtempDisposableSync` — module
- `mkdtempSync` — module
- `open` — module
- `openAsBlob` — module
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

## `fs/promises`

### Methods

- `access` — module
- `appendFile` — module
- `chmod` — module
- `chown` — module
- `copyFile` — module
- `cp` — module
- `glob` — module
- `lchmod` — module
- `lchown` — module
- `link` — module
- `lstat` — module
- `lutimes` — module
- `mkdir` — module
- `mkdtemp` — module
- `mkdtempDisposable` — module
- `open` — module
- `opendir` — module
- `pull` — instance *(class: `FileHandle`)*
- `pullSync` — instance *(class: `FileHandle`)*
- `readFile` — module
- `readdir` — module
- `readlink` — module
- `realpath` — module
- `rename` — module
- `rm` — module
- `rmdir` — module
- `stat` — module
- `statfs` — module
- `symlink` — module
- `truncate` — module
- `unlink` — module
- `utimes` — module
- `watch` — module
- `writeFile` — module
- `writer` — instance *(class: `FileHandle`)*

### Properties

- `constants`
- `default`

## `http`

### Classes

- `Agent`
- `ClientRequest`
- `IncomingMessage`
- `IncomingMessage`
- `OutgoingMessage`
- `OutgoingMessage`
- `Server`
- `Server`
- `ServerResponse`
- `ServerResponse`
- `WebSocket`

### Methods

- `Agent` — module
- `Server` — module
- `__get_aborted` — instance *(class: `ClientRequest`)*
- `__get_aborted` — instance *(class: `IncomingMessage`)*
- `__get_complete` — instance *(class: `IncomingMessage`)*
- `__get_connection` — instance *(class: `ClientRequest`)*
- `__get_createConnection` — instance *(class: `Agent`)*
- `__get_createSocket` — instance *(class: `Agent`)*
- `__get_defaultPort` — instance *(class: `Agent`)*
- `__get_destroyed` — instance *(class: `Agent`)*
- `__get_destroyed` — instance *(class: `ClientRequest`)*
- `__get_destroyed` — instance *(class: `IncomingMessage`)*
- `__get_finished` — instance *(class: `ClientRequest`)*
- `__get_freeSockets` — instance *(class: `Agent`)*
- `__get_headers` — instance *(class: `IncomingMessage`)*
- `__get_headersSent` — instance *(class: `ServerResponse`)*
- `__get_headersTimeout` — instance *(class: `HttpServer`)*
- `__get_host` — instance *(class: `ClientRequest`)*
- `__get_httpVersion` — instance *(class: `IncomingMessage`)*
- `__get_httpVersionMajor` — instance *(class: `IncomingMessage`)*
- `__get_httpVersionMinor` — instance *(class: `IncomingMessage`)*
- `__get_keepAlive` — instance *(class: `Agent`)*
- `__get_keepAliveMsecs` — instance *(class: `Agent`)*
- `__get_keepAliveTimeout` — instance *(class: `HttpServer`)*
- `__get_keepAliveTimeoutBuffer` — instance *(class: `HttpServer`)*
- `__get_listening` — instance *(class: `HttpServer`)*
- `__get_maxFreeSockets` — instance *(class: `Agent`)*
- `__get_maxHeadersCount` — instance *(class: `HttpServer`)*
- `__get_maxHeadersCount` — instance *(class: `ClientRequest`)*
- `__get_maxRequestsPerSocket` — instance *(class: `HttpServer`)*
- `__get_maxSockets` — instance *(class: `Agent`)*
- `__get_maxTotalSockets` — instance *(class: `Agent`)*
- `__get_method` — instance *(class: `ClientRequest`)*
- `__get_method` — instance *(class: `IncomingMessage`)*
- `__get_path` — instance *(class: `ClientRequest`)*
- `__get_protocol` — instance *(class: `Agent`)*
- `__get_protocol` — instance *(class: `ClientRequest`)*
- `__get_requestTimeout` — instance *(class: `HttpServer`)*
- `__get_requests` — instance *(class: `Agent`)*
- `__get_reusedSocket` — instance *(class: `ClientRequest`)*
- `__get_socket` — instance *(class: `ClientRequest`)*
- `__get_sockets` — instance *(class: `Agent`)*
- `__get_statusCode` — instance *(class: `IncomingMessage`)*
- `__get_statusCode` — instance *(class: `ServerResponse`)*
- `__get_statusMessage` — instance *(class: `IncomingMessage`)*
- `__get_timeout` — instance *(class: `HttpServer`)*
- `__get_trailers` — instance *(class: `IncomingMessage`)*
- `__get_url` — instance *(class: `IncomingMessage`)*
- `__get_writableEnded` — instance *(class: `ClientRequest`)*
- `__get_writableEnded` — instance *(class: `ServerResponse`)*
- `__get_writableFinished` — instance *(class: `ClientRequest`)*
- `__get_writableFinished` — instance *(class: `ServerResponse`)*
- `__set_createConnection` — instance *(class: `Agent`)*
- `__set_createSocket` — instance *(class: `Agent`)*
- `__set_headersTimeout` — instance *(class: `HttpServer`)*
- `__set_keepAlive` — instance *(class: `Agent`)*
- `__set_keepAliveMsecs` — instance *(class: `Agent`)*
- `__set_keepAliveTimeout` — instance *(class: `HttpServer`)*
- `__set_keepAliveTimeoutBuffer` — instance *(class: `HttpServer`)*
- `__set_maxFreeSockets` — instance *(class: `Agent`)*
- `__set_maxHeadersCount` — instance *(class: `HttpServer`)*
- `__set_maxRequestsPerSocket` — instance *(class: `HttpServer`)*
- `__set_maxSockets` — instance *(class: `Agent`)*
- `__set_maxTotalSockets` — instance *(class: `Agent`)*
- `__set_protocol` — instance *(class: `Agent`)*
- `__set_requestTimeout` — instance *(class: `HttpServer`)*
- `__set_sendDate` — instance *(class: `ServerResponse`)*
- `__set_statusCode` — instance *(class: `ServerResponse`)*
- `__set_statusMessage` — instance *(class: `ServerResponse`)*
- `__set_strictContentLength` — instance *(class: `ServerResponse`)*
- `__set_timeout` — instance *(class: `HttpServer`)*
- `_connectionListener` — module
- `abort` — instance *(class: `ClientRequest`)*
- `addListener` — instance *(class: `HttpServer`)*
- `addListener` — instance *(class: `IncomingMessage`)*
- `addListener` — instance *(class: `ServerResponse`)*
- `addTrailers` — instance *(class: `ServerResponse`)*
- `address` — instance *(class: `HttpServer`)*
- `appendHeader` — instance *(class: `ServerResponse`)*
- `close` — instance *(class: `Agent`)*
- `close` — instance *(class: `HttpServer`)*
- `closeAllConnections` — instance *(class: `HttpServer`)*
- `closeIdleConnections` — instance *(class: `HttpServer`)*
- `cork` — instance *(class: `ClientRequest`)*
- `cork` — instance *(class: `ServerResponse`)*
- `createServer` — module
- `createServer` — module
- `defaultPort` — instance *(class: `Agent`)*
- `destroy` — instance *(class: `Agent`)*
- `destroy` — instance *(class: `IncomingMessage`)*
- `destroy` — instance *(class: `ClientRequest`)*
- `destroyed` — instance *(class: `Agent`)*
- `end` — instance *(class: `ServerResponse`)*
- `flushHeaders` — instance *(class: `ClientRequest`)*
- `flushHeaders` — instance *(class: `ServerResponse`)*
- `freeSockets` — instance *(class: `Agent`)*
- `get` — module
- `getHeader` — instance *(class: `ClientRequest`)*
- `getHeader` — instance *(class: `ServerResponse`)*
- `getHeaderNames` — instance *(class: `ClientRequest`)*
- `getHeaderNames` — instance *(class: `ServerResponse`)*
- `getHeaders` — instance *(class: `ClientRequest`)*
- `getHeaders` — instance *(class: `ServerResponse`)*
- `getName` — instance *(class: `Agent`)*
- `getRawHeaderNames` — instance *(class: `ClientRequest`)*
- `getStatus` — instance *(class: `ServerResponse`)*
- `hasHeader` — instance *(class: `ClientRequest`)*
- `hasHeader` — instance *(class: `ServerResponse`)*
- `headers` — instance *(class: `IncomingMessage`)*
- `headersTimeout` — instance *(class: `HttpServer`)*
- `httpVersion` — instance *(class: `IncomingMessage`)*
- `keepAlive` — instance *(class: `Agent`)*
- `keepAliveMsecs` — instance *(class: `Agent`)*
- `keepAliveTimeout` — instance *(class: `HttpServer`)*
- `keepAliveTimeoutBuffer` — instance *(class: `HttpServer`)*
- `keepSocketAlive` — instance *(class: `Agent`)* ⚠ **stub** — reqwest owns the keep-alive pool; per-socket hooks are no-ops, warns once (#4917)
- `listen` — instance *(class: `HttpServer`)*
- `listenerCount` — instance *(class: `ClientRequest`)*
- `listening` — instance *(class: `HttpServer`)*
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
- `ref` — instance *(class: `HttpServer`)*
- `removeHeader` — instance *(class: `ClientRequest`)*
- `removeHeader` — instance *(class: `ServerResponse`)*
- `request` — module
- `requestTimeout` — instance *(class: `HttpServer`)*
- `requests` — instance *(class: `Agent`)*
- `resume` — instance *(class: `IncomingMessage`)*
- `reuseSocket` — instance *(class: `Agent`)* ⚠ **stub** — reqwest owns the keep-alive pool; per-socket hooks are no-ops, warns once (#4917)
- `setEncoding` — instance *(class: `IncomingMessage`)*
- `setGlobalProxyFromEnv` — module
- `setHeader` — instance *(class: `ClientRequest`)*
- `setHeader` — instance *(class: `ServerResponse`)*
- `setHeaders` — instance *(class: `ServerResponse`)*
- `setMaxIdleHTTPParsers` — module
- `setNoDelay` — instance *(class: `ClientRequest`)*
- `setSocketKeepAlive` — instance *(class: `ClientRequest`)*
- `setStatus` — instance *(class: `ServerResponse`)*
- `setTimeout` — instance *(class: `HttpServer`)*
- `setTimeout` — instance *(class: `IncomingMessage`)*
- `setTimeout` — instance *(class: `ClientRequest`)*
- `setTimeout` — instance *(class: `ServerResponse`)*
- `sockets` — instance *(class: `Agent`)*
- `statusCode` — instance *(class: `IncomingMessage`)*
- `statusMessage` — instance *(class: `IncomingMessage`)*
- `timeout` — instance *(class: `HttpServer`)*
- `trailers` — instance *(class: `IncomingMessage`)*
- `uncork` — instance *(class: `ClientRequest`)*
- `uncork` — instance *(class: `ServerResponse`)*
- `unref` — instance *(class: `HttpServer`)*
- `url` — instance *(class: `IncomingMessage`)*
- `validateHeaderName` — module
- `validateHeaderValue` — module
- `write` — instance *(class: `ServerResponse`)*
- `writeContinue` — instance *(class: `ServerResponse`)*
- `writeEarlyHints` — instance *(class: `ServerResponse`)*
- `writeHead` — instance *(class: `ServerResponse`)*
- `writeProcessing` — instance *(class: `ServerResponse`)*

### Properties

- `METHODS`
- `STATUS_CODES`
- `globalAgent`
- `kConnectionsCheckingInterval`
- `maxHeaderSize`

## `http2`

### Classes

- `Http2ServerRequest`
- `Http2ServerResponse`

### Methods

- `connect` — module
- `createSecureServer` — module
- `createServer` — module
- `getDefaultSettings` — module
- `getPackedSettings` — module
- `getUnpackedSettings` — module
- `performServerHandshake` — module

### Properties

- `constants`
- `default`
- `sensitiveHeaders`

## `https`

### Classes

- `Agent`
- `Server`
- `Server`

### Methods

- `Agent` — module
- `Server` — module
- `__get_headersTimeout` — instance *(class: `HttpsServer`)*
- `__get_keepAliveTimeout` — instance *(class: `HttpsServer`)*
- `__get_keepAliveTimeoutBuffer` — instance *(class: `HttpsServer`)*
- `__get_listening` — instance *(class: `HttpsServer`)*
- `__get_maxHeadersCount` — instance *(class: `HttpsServer`)*
- `__get_maxRequestsPerSocket` — instance *(class: `HttpsServer`)*
- `__get_requestTimeout` — instance *(class: `HttpsServer`)*
- `__get_timeout` — instance *(class: `HttpsServer`)*
- `__set_headersTimeout` — instance *(class: `HttpsServer`)*
- `__set_keepAliveTimeout` — instance *(class: `HttpsServer`)*
- `__set_keepAliveTimeoutBuffer` — instance *(class: `HttpsServer`)*
- `__set_maxHeadersCount` — instance *(class: `HttpsServer`)*
- `__set_maxRequestsPerSocket` — instance *(class: `HttpsServer`)*
- `__set_requestTimeout` — instance *(class: `HttpsServer`)*
- `__set_timeout` — instance *(class: `HttpsServer`)*
- `addListener` — instance *(class: `HttpsServer`)*
- `address` — instance *(class: `HttpsServer`)*
- `close` — instance *(class: `HttpsServer`)*
- `closeAllConnections` — instance *(class: `HttpsServer`)*
- `closeIdleConnections` — instance *(class: `HttpsServer`)*
- `createServer` — module
- `createServer` — module
- `get` — module
- `headersTimeout` — instance *(class: `HttpsServer`)*
- `keepAliveTimeout` — instance *(class: `HttpsServer`)*
- `keepAliveTimeoutBuffer` — instance *(class: `HttpsServer`)*
- `listen` — instance *(class: `HttpsServer`)*
- `listening` — instance *(class: `HttpsServer`)*
- `maxHeadersCount` — instance *(class: `HttpsServer`)*
- `maxRequestsPerSocket` — instance *(class: `HttpsServer`)*
- `on` — instance *(class: `HttpsServer`)*
- `ref` — instance *(class: `HttpsServer`)*
- `request` — module
- `requestTimeout` — instance *(class: `HttpsServer`)*
- `setTimeout` — instance *(class: `HttpsServer`)*
- `timeout` — instance *(class: `HttpsServer`)*
- `unref` — instance *(class: `HttpsServer`)*

### Properties

- `globalAgent`

## `inspector`

### Classes

- `Session`

### Methods

- `Session` — module
- `close` — module
- `connect` — instance *(class: `Session`)*
- `connectToMainThread` — instance *(class: `Session`)*
- `disconnect` — instance *(class: `Session`)*
- `on` — instance *(class: `Session`)*
- `once` — instance *(class: `Session`)*
- `open` — module ⚠ **stub** — accepts port/host but binds no real WebSocket inspector endpoint; sessions are in-process fakes (#4916)
- `post` — instance *(class: `Session`)* ⚠ **stub** — only Runtime.enable and a canned Runtime.evaluate subset respond; every other protocol method throws Inspector error -32601 (#4916)
- `url` — module ⚠ **stub** — always undefined: Perry never exposes a real inspector endpoint (#4916)
- `waitForDebugger` — module ⚠ **stub** — returns immediately after open(); there is no debugger to wait for (#4916)

### Properties

- `Network`
- `console`
- `default`

## `inspector/promises`

### Classes

- `Session`

### Methods

- `Session` — module
- `connect` — instance *(class: `Session`)*
- `connectToMainThread` — instance *(class: `Session`)*
- `disconnect` — instance *(class: `Session`)*
- `on` — instance *(class: `Session`)*
- `once` — instance *(class: `Session`)*
- `post` — instance *(class: `Session`)*

### Properties

- `default`

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

## `module`

### Classes

- `Module`
- `SourceMap`

### Methods

- `Module` — module
- `SourceMap` — module
- `_findPath` — module
- `_initPaths` — module
- `_load` — module
- `_nodeModulePaths` — module
- `_preloadModules` — module
- `_resolveFilename` — module
- `_resolveLookupPaths` — module
- `createRequire` — module
- `enableCompileCache` — module
- `findPackageJSON` — module
- `findSourceMap` — module
- `flushCompileCache` — module
- `getCompileCacheDir` — module
- `getSourceMapsSupport` — module
- `isBuiltin` — module
- `register` — module
- `registerHooks` — module
- `runMain` — module
- `setSourceMapsSupport` — module
- `stripTypeScriptTypes` — module
- `syncBuiltinESMExports` — module

### Properties

- `Module`
- `_cache`
- `_extensions`
- `_pathCache`
- `builtinModules`
- `constants`
- `default`
- `globalPaths`

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

- `BlockList`
- `Server`
- `Socket`
- `SocketAddress`
- `Stream`

### Methods

- `BlockList` — module
- `Server` — module
- `Socket` — module
- `SocketAddress` — module
- `Stream` — module
- `__set_dropMaxConnection` — instance *(class: `Server`)*
- `__set_maxConnections` — instance *(class: `Server`)*
- `_createServerHandle` — module
- `_normalizeArgs` — module
- `addAddress` — instance *(class: `BlockList`)*
- `addListener` — instance *(class: `Socket`)*
- `addListener` — instance *(class: `Server`)*
- `addRange` — instance *(class: `BlockList`)*
- `addSubnet` — instance *(class: `BlockList`)*
- `address` — instance *(class: `Socket`)*
- `address` — instance *(class: `SocketAddress`)*
- `address` — instance *(class: `Server`)*
- `autoSelectFamilyAttemptedAddresses` — instance *(class: `Socket`)*
- `bufferSize` — instance *(class: `Socket`)*
- `bytesRead` — instance *(class: `Socket`)*
- `bytesWritten` — instance *(class: `Socket`)*
- `check` — instance *(class: `BlockList`)*
- `close` — instance *(class: `Server`)*
- `connect` — module
- `connect` — instance *(class: `Socket`)*
- `connecting` — instance *(class: `Socket`)*
- `cork` — instance *(class: `Socket`)*
- `createConnection` — module
- `createServer` — module
- `destroy` — instance *(class: `Socket`)*
- `destroyed` — instance *(class: `Socket`)*
- `dropMaxConnection` — instance *(class: `Server`)*
- `end` — instance *(class: `Socket`)*
- `eventNames` — instance *(class: `Socket`)*
- `eventNames` — instance *(class: `Server`)*
- `exportKeyingMaterial` — instance *(class: `Socket`)*
- `family` — instance *(class: `SocketAddress`)*
- `flowlabel` — instance *(class: `SocketAddress`)*
- `fromJSON` — instance *(class: `BlockList`)*
- `getCertificate` — instance *(class: `Socket`)*
- `getCipher` — instance *(class: `Socket`)*
- `getConnections` — instance *(class: `Server`)*
- `getDefaultAutoSelectFamily` — module
- `getDefaultAutoSelectFamilyAttemptTimeout` — module
- `getPeerCertificate` — instance *(class: `Socket`)*
- `getProtocol` — instance *(class: `Socket`)*
- `getSession` — instance *(class: `Socket`)*
- `getTypeOfService` — instance *(class: `Socket`)*
- `isBlockList` — module *(class: `BlockList`)*
- `isIP` — module
- `isIPv4` — module
- `isIPv6` — module
- `isSessionReused` — instance *(class: `Socket`)*
- `listen` — instance *(class: `Server`)*
- `listenerCount` — instance *(class: `Socket`)*
- `listenerCount` — instance *(class: `Server`)*
- `listeners` — instance *(class: `Socket`)*
- `listeners` — instance *(class: `Server`)*
- `listening` — instance *(class: `Server`)*
- `localAddress` — instance *(class: `Socket`)*
- `localFamily` — instance *(class: `Socket`)*
- `localPort` — instance *(class: `Socket`)*
- `maxConnections` — instance *(class: `Server`)*
- `off` — instance *(class: `Socket`)*
- `off` — instance *(class: `Server`)*
- `on` — instance *(class: `Socket`)*
- `once` — instance *(class: `Socket`)*
- `once` — instance *(class: `Server`)*
- `parse` — module *(class: `SocketAddress`)*
- `pause` — instance *(class: `Socket`)*
- `pending` — instance *(class: `Socket`)*
- `port` — instance *(class: `SocketAddress`)*
- `rawListeners` — instance *(class: `Socket`)*
- `rawListeners` — instance *(class: `Server`)*
- `readyState` — instance *(class: `Socket`)*
- `ref` — instance *(class: `Socket`)*
- `remoteAddress` — instance *(class: `Socket`)*
- `remoteFamily` — instance *(class: `Socket`)*
- `remotePort` — instance *(class: `Socket`)*
- `removeAllListeners` — instance *(class: `Socket`)*
- `removeAllListeners` — instance *(class: `Server`)*
- `removeListener` — instance *(class: `Socket`)*
- `removeListener` — instance *(class: `Server`)*
- `resetAndDestroy` — instance *(class: `Socket`)*
- `resume` — instance *(class: `Socket`)*
- `rules` — instance *(class: `BlockList`)*
- `setDefaultAutoSelectFamily` — module
- `setDefaultAutoSelectFamilyAttemptTimeout` — module
- `setDefaultEncoding` — instance *(class: `Socket`)*
- `setEncoding` — instance *(class: `Socket`)*
- `setKeepAlive` — instance *(class: `Socket`)*
- `setMaxSendFragment` — instance *(class: `Socket`)*
- `setNoDelay` — instance *(class: `Socket`)*
- `setTimeout` — instance *(class: `Socket`)*
- `setTypeOfService` — instance *(class: `Socket`)*
- `timeout` — instance *(class: `Socket`)*
- `toJSON` — instance *(class: `BlockList`)*
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
- `FormData`
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
- `default`
- `devNull`

## `path`

### Methods

- `_makeLong` — module
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

- `default`
- `delimiter`
- `posix`
- `sep`
- `win32`

## `path/posix`

### Methods

- `_makeLong` — module
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

- `default`
- `delimiter`
- `posix`
- `sep`
- `win32`

## `path/win32`

### Methods

- `_makeLong` — module
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

- `default`
- `delimiter`
- `posix`
- `sep`
- `win32`

## `perf_hooks`

### Classes

- `Performance`
- `PerformanceEntry`
- `PerformanceMark`
- `PerformanceMeasure`
- `PerformanceObserver`
- `PerformanceObserverEntryList`
- `PerformanceResourceTiming`

### Methods

- `createHistogram` — module
- `disconnect` — instance *(class: `PerformanceObserver`)*
- `monitorEventLoopDelay` — module
- `observe` — instance *(class: `PerformanceObserver`)*
- `takeRecords` — instance *(class: `PerformanceObserver`)*
- `timerify` — module

### Properties

- `constants`
- `performance`

## `perry`

### Methods

- `embeddedFiles` — module
- `readEmbedded` — module

### Properties

- `isStandaloneExecutable`

## `perry/ads`

### Methods

- `js_ads_banner_create` — module
- `js_ads_banner_destroy` — module
- `js_ads_interstitial_load` — module
- `js_ads_interstitial_show` — module
- `js_ads_request_consent` — module
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

## `perry/compose`

### Methods

- `config` — module
- `down` — module
- `exec` — module
- `logs` — module
- `ps` — module
- `restart` — module
- `start` — module
- `stop` — module
- `up` — module

## `perry/container`

### Methods

- `composeUp` — module
- `create` — module
- `detectBackend` — module
- `downAll` — module
- `downByProject` — module
- `exec` — module
- `getAvailableBackends` — module
- `getBackend` — module
- `getBackendPriority` — module
- `inspect` — module
- `list` — module
- `listImages` — module
- `logs` — module
- `pullImage` — module
- `remove` — module
- `removeIfExists` — module
- `removeImage` — module
- `run` — module
- `selectBackendFor` — module
- `setBackend` — module
- `setBackends` — module
- `start` — module
- `stop` — module

## `perry/container-compose`

### Methods

- `config` — module
- `down` — module
- `exec` — module
- `logs` — module
- `ps` — module
- `restart` — module
- `start` — module
- `stop` — module
- `up` — module

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
- `getSafeAreaInsets` — module
- `hapticPlay` — module
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
- `BloomView` — module
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
- `appSetActivationPolicy` — module
- `appSetMaxSize` — module
- `appSetMinSize` — module
- `appSetTimer` — module
- `attributedTextAppend` — module
- `attributedTextClear` — module
- `bloomViewGetHwnd` — module
- `bloomViewGetNativeHandle` — module
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

## `perry/workloads`

### Methods

- `graph` — module
- `inspectGraph` — module
- `node` — module
- `runGraph` — module

### Properties

- `policy`
- `runtime`

## `perry/yoga`

### Methods

- `calculateLayout` — module
- `childCount` — module
- `getComputed` — module
- `getComputedEdge` — module
- `insertChild` — module
- `nodeFree` — module
- `nodeNew` — module
- `removeChild` — module
- `setEdge` — module
- `setEnum` — module
- `setGap` — module
- `setMeasureFunc` — module
- `setNumber` — module
- `unsetMeasureFunc` — module

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

- `_debugEnd` — module
- `_debugProcess` — module
- `_fatalException` — module
- `_getActiveHandles` — module
- `_getActiveRequests` — module
- `_kill` — module
- `_linkedBinding` — module
- `_rawDebug` — module
- `_startProfilerIdleNotifier` — module
- `_stopProfilerIdleNotifier` — module
- `_tickCallback` — module
- `abort` — module
- `addUncaughtExceptionCaptureCallback` — module
- `availableMemory` — module
- `binding` — module
- `chdir` — module
- `constrainedMemory` — module
- `cpuUsage` — module
- `cwd` — module
- `dlopen` — module
- `emitWarning` — module
- `execve` — module
- `exit` — module
- `getActiveResourcesInfo` — module
- `getBuiltinModule` — module
- `getegid` — module
- `geteuid` — module
- `getgid` — module
- `getgroups` — module
- `getuid` — module
- `hasUncaughtExceptionCaptureCallback` — module
- `hrtime` — module
- `initgroups` — module
- `kill` — module
- `loadEnvFile` — module
- `memoryUsage` — module
- `nextTick` — module
- `openStdin` — module
- `reallyExit` — module
- `ref` — module
- `resourceUsage` — module
- `setSourceMapsEnabled` — module
- `setSourceMapsEnabled` — module
- `setUncaughtExceptionCaptureCallback` — module
- `setegid` — module
- `seteuid` — module
- `setgid` — module
- `setgroups` — module
- `setuid` — module
- `sourceMapsEnabled` — module
- `sourceMapsEnabled` — module
- `threadCpuUsage` — module
- `umask` — module
- `unref` — module
- `uptime` — module

### Properties

- `_eval`
- `_events`
- `_eventsCount`
- `_exiting`
- `_maxListeners`
- `_preload_modules`
- `allowedNodeEnvironmentFlags`
- `arch`
- `argv`
- `argv0`
- `config`
- `debugPort`
- `domain`
- `env`
- `execArgv`
- `execPath`
- `features`
- `finalization`
- `moduleLoadList`
- `permission`
- `pid`
- `platform`
- `ppid`
- `release`
- `report`
- `stderr`
- `stdin`
- `stdout`
- `title`
- `version`
- `versions`

## `punycode`

### Methods

- `decode` — module
- `encode` — module
- `toASCII` — module
- `toUnicode` — module

### Properties

- `default`
- `ucs2`
- `version`

## `querystring`

### Methods

- `decode` — module
- `encode` — module
- `escape` — module
- `parse` — module
- `stringify` — module
- `unescape` — module
- `unescapeBuffer` — module

### Properties

- `default`

## `rate-limiter-flexible`

### Classes

- `RateLimiterAbstract`
- `RateLimiterMemory`

## `readline`

### Methods

- `clearLine` — module
- `clearScreenDown` — module
- `close` — instance
- `createInterface` — module
- `cursorTo` — module
- `emitKeypressEvents` — module
- `getCursorPos` — instance
- `getPrompt` — instance
- `iterator` — instance
- `line` — instance
- `moveCursor` — module
- `on` — instance
- `pause` — instance
- `prompt` — instance
- `question` — instance
- `resume` — instance
- `setPrompt` — instance
- `terminal` — instance
- `write` — instance

## `readline/promises`

### Classes

- `Interface`
- `Readline`

### Methods

- `close` — instance
- `createInterface` — module
- `question` — instance

## `redis`

### Classes

- `Redis`

### Methods

- `createClient` — module

## `repl`

### Classes

- `REPLServer`
- `Recoverable`

### Methods

- `REPLServer` — module ⚠ **stub** — REPLServer shape only: never reads the input stream, and .write() evaluates just numeric literals, context lookups, and a single '+'; no real JS eval loop (#4916)
- `Recoverable` — module
- `addListener` — instance *(class: `REPLServer`)*
- `clearBufferedCommand` — instance *(class: `REPLServer`)*
- `defineCommand` — instance *(class: `REPLServer`)*
- `displayPrompt` — instance *(class: `REPLServer`)*
- `emit` — instance *(class: `REPLServer`)*
- `on` — instance *(class: `REPLServer`)*
- `once` — instance *(class: `REPLServer`)*
- `setupHistory` — instance *(class: `REPLServer`)*
- `start` — module ⚠ **stub** — REPLServer shape only: never reads the input stream, and .write() evaluates just numeric literals, context lookups, and a single '+'; no real JS eval loop (#4916)
- `write` — instance *(class: `REPLServer`)*

### Properties

- `REPL_MODE_SLOPPY`
- `REPL_MODE_STRICT`
- `builtinModules`
- `default`

## `sea`

### Methods

- `getAsset` — module
- `getAssetAsBlob` — module
- `getAssetKeys` — module
- `getRawAsset` — module
- `isSea` — module

### Properties

- `default`

## `sharp`

### Methods

- `autoOrient` — instance
- `avif` — instance
- `blur` — instance
- `composite` — instance
- `default` — module
- `extend` — instance
- `extract` — instance
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
- `sharpen` — instance
- `toBuffer` — instance
- `toFile` — instance
- `trim` — instance
- `webp` — instance
- `width` — instance

## `slugify`

### Methods

- `default` — module
- `slugify` — module

## `sqlite`

### Classes

- `DatabaseSync`
- `SQLTagStore`
- `Session`
- `StatementSync`

### Methods

- `@@__perry_wk_dispose` — instance
- `DatabaseSync` — module
- `Session` — module
- `StatementSync` — module
- `__perry_dispose__` — instance
- `aggregate` — instance *(class: `DatabaseSync`)*
- `all` — instance *(class: `SQLTagStore`)*
- `all` — instance
- `applyChangeset` — instance
- `backup` — module
- `capacity` — instance *(class: `SQLTagStore`)*
- `changeset` — instance
- `clear` — instance *(class: `SQLTagStore`)*
- `close` — instance
- `columns` — instance
- `createSession` — instance
- `createTagStore` — instance *(class: `DatabaseSync`)*
- `db` — instance *(class: `SQLTagStore`)*
- `enableDefensive` — instance *(class: `DatabaseSync`)*
- `enableLoadExtension` — instance
- `exec` — instance
- `expandedSQL` — instance
- `function` — instance *(class: `DatabaseSync`)*
- `get` — instance *(class: `SQLTagStore`)*
- `get` — instance
- `isOpen` — instance
- `isTransaction` — instance
- `iterate` — instance *(class: `SQLTagStore`)*
- `iterate` — instance
- `limits` — instance
- `loadExtension` — instance
- `location` — instance
- `open` — instance
- `patchset` — instance
- `prepare` — instance
- `run` — instance *(class: `SQLTagStore`)*
- `run` — instance
- `setAllowBareNamedParameters` — instance
- `setAllowUnknownNamedParameters` — instance
- `setAuthorizer` — instance *(class: `DatabaseSync`)*
- `setReadBigInts` — instance
- `setReturnArrays` — instance
- `size` — instance *(class: `SQLTagStore`)*
- `sourceSQL` — instance

### Properties

- `constants`

## `stream`

### Classes

- `Duplex`
- `PassThrough`
- `Readable`
- `Stream`
- `Transform`
- `Writable`

### Methods

- `_isArrayBufferView` — module
- `_isUint8Array` — module
- `_uint8ArrayToBuffer` — module
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
- `getDefaultHighWaterMark` — module
- `getMaxListeners` — instance
- `isDestroyed` — module
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
- `uncork` — instance
- `unpipe` — instance
- `unshift` — instance
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

## `stream/consumers`

### Methods

- `arrayBuffer` — module
- `blob` — module
- `buffer` — module
- `bytes` — module
- `json` — module
- `text` — module

### Properties

- `default`

## `stream/promises`

### Methods

- `finished` — module
- `finished` — module
- `pipeline` — module
- `pipeline` — module

## `stream/web`

### Classes

- `ByteLengthQueuingStrategy`
- `CompressionStream`
- `CountQueuingStrategy`
- `DecompressionStream`
- `ReadableByteStreamController`
- `ReadableStream`
- `ReadableStreamBYOBReader`
- `ReadableStreamBYOBRequest`
- `ReadableStreamDefaultController`
- `ReadableStreamDefaultReader`
- `TextDecoderStream`
- `TextEncoderStream`
- `TransformStream`
- `TransformStreamDefaultController`
- `WritableStream`
- `WritableStreamDefaultController`
- `WritableStreamDefaultWriter`

### Properties

- `default`

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

## `sys`

### Classes

- `MIMEParams`
- `MIMEType`
- `TextDecoder`
- `TextEncoder`

### Methods

- `MIMEParams` — module
- `MIMEType` — module
- `_errnoException` — module
- `_exceptionWithHostPort` — module
- `_extend` — module
- `aborted` — module
- `callbackify` — module
- `convertProcessSignalToExitCode` — module
- `debug` — module
- `debuglog` — module
- `deprecate` — module
- `diff` — module
- `format` — module
- `formatWithOptions` — module
- `getCallSites` — module
- `getSystemErrorMap` — module
- `getSystemErrorMessage` — module
- `getSystemErrorName` — module
- `inherits` — module
- `inspect` — module
- `isArray` — module
- `isDeepStrictEqual` — module
- `parseArgs` — module
- `parseEnv` — module
- `promisify` — module
- `setTraceSigInt` — module
- `stripVTControlCharacters` — module
- `styleText` — module
- `toUSVString` — module
- `transferableAbortController` — module
- `transferableAbortSignal` — module

### Properties

- `default`
- `types`

## `test`

### Methods

- `after` — module
- `afterEach` — module
- `before` — module
- `beforeEach` — module
- `default` — module
- `describe` — module
- `enable` — module *(class: `timers`)*
- `expectFailure` — module
- `fn` — module *(class: `mock`)*
- `getter` — module *(class: `mock`)*
- `it` — module
- `method` — module *(class: `mock`)*
- `only` — module
- `property` — module *(class: `mock`)*
- `reset` — module *(class: `mock`)*
- `restoreAll` — module *(class: `mock`)*
- `run` — module
- `runAll` — module *(class: `timers`)*
- `setDefaultSnapshotSerializers` — module *(class: `snapshot`)*
- `setResolveSnapshotPath` — module *(class: `snapshot`)*
- `setTime` — module *(class: `timers`)*
- `setter` — module *(class: `mock`)*
- `skip` — module
- `suite` — module
- `test` — module
- `tick` — module *(class: `timers`)*
- `todo` — module

### Properties

- `assert`
- `mock`
- `snapshot`

## `test/reporters`

### Methods

- `dot` — module
- `junit` — module
- `lcov` — module
- `spec` — module
- `tap` — module

### Properties

- `default`

## `timers`

### Methods

- `clearImmediate` — module
- `clearInterval` — module
- `clearTimeout` — module
- `setImmediate` — module
- `setInterval` — module
- `setTimeout` — module

### Properties

- `promises`

## `timers/promises`

### Methods

- `setImmediate` — module
- `setInterval` — module
- `setTimeout` — module

### Properties

- `scheduler`

## `tls`

### Classes

- `SecureContext`

### Methods

- `SecureContext` — module
- `Server` — module
- `TLSSocket` — module
- `addListener` — instance *(class: `Server`)*
- `address` — instance *(class: `Server`)*
- `checkServerIdentity` — module
- `close` — instance *(class: `Server`)*
- `connect` — module
- `createSecureContext` — module
- `createServer` — module
- `eventNames` — instance *(class: `Server`)*
- `getCACertificates` — module
- `getCiphers` — module
- `getTicketKeys` — instance *(class: `Server`)*
- `listen` — instance *(class: `Server`)*
- `listenerCount` — instance *(class: `Server`)*
- `off` — instance *(class: `Server`)*
- `on` — instance *(class: `Server`)*
- `once` — instance *(class: `Server`)*
- `removeAllListeners` — instance *(class: `Server`)*
- `removeListener` — instance *(class: `Server`)*
- `setDefaultCACertificates` — module
- `setSecureContext` — instance *(class: `Server`)*
- `setTicketKeys` — instance *(class: `Server`)*

### Properties

- `CLIENT_RENEG_LIMIT`
- `CLIENT_RENEG_WINDOW`
- `DEFAULT_CIPHERS`
- `DEFAULT_ECDH_CURVE`
- `DEFAULT_MAX_VERSION`
- `DEFAULT_MIN_VERSION`
- `rootCertificates`

## `tty`

### Classes

- `ReadStream`
- `WriteStream`

### Methods

- `ReadStream` — module
- `WriteStream` — module
- `_refreshSize` — instance *(class: `WriteStream`)*
- `addListener` — instance *(class: `WriteStream`)*
- `clearLine` — instance *(class: `WriteStream`)*
- `clearScreenDown` — instance *(class: `WriteStream`)*
- `cursorTo` — instance *(class: `WriteStream`)*
- `getColorDepth` — instance *(class: `WriteStream`)*
- `getWindowSize` — instance *(class: `WriteStream`)*
- `hasColors` — instance *(class: `WriteStream`)*
- `isatty` — module
- `moveCursor` — instance *(class: `WriteStream`)*
- `off` — instance *(class: `WriteStream`)*
- `on` — instance *(class: `WriteStream`)*
- `once` — instance *(class: `WriteStream`)*
- `removeAllListeners` — instance *(class: `WriteStream`)*
- `removeListener` — instance *(class: `WriteStream`)*
- `setRawMode` — instance *(class: `ReadStream`)*

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
- `URLPattern`
- `URLSearchParams`
- `Url`

### Methods

- `Url` — module
- `domainToASCII` — module
- `domainToUnicode` — module
- `exec` — instance *(class: `URLPattern`)*
- `fileURLToPath` — module
- `fileURLToPathBuffer` — module
- `format` — module
- `parse` — module
- `pathToFileURL` — module
- `resolve` — module
- `resolveObject` — module
- `test` — instance *(class: `URLPattern`)*
- `urlToHttpOptions` — module

### Properties

- `default`

## `util`

### Classes

- `MIMEParams`
- `MIMEType`
- `TextDecoder`
- `TextEncoder`

### Methods

- `MIMEParams` — module
- `MIMEType` — module
- `_errnoException` — module
- `_exceptionWithHostPort` — module
- `_extend` — module
- `aborted` — module
- `callbackify` — module
- `convertProcessSignalToExitCode` — module
- `debug` — module
- `debuglog` — module
- `deprecate` — module
- `diff` — module
- `format` — module
- `formatWithOptions` — module
- `getCallSites` — module
- `getSystemErrorMap` — module
- `getSystemErrorMessage` — module
- `getSystemErrorName` — module
- `inherits` — module
- `inspect` — module
- `isArray` — module
- `isDeepStrictEqual` — module
- `parseArgs` — module
- `parseEnv` — module
- `promisify` — module
- `setTraceSigInt` — module
- `stripVTControlCharacters` — module
- `styleText` — module
- `toUSVString` — module
- `transferableAbortController` — module
- `transferableAbortSignal` — module

### Properties

- `default`
- `types`

## `util/types`

### Methods

- `isAnyArrayBuffer` — module
- `isArgumentsObject` — module
- `isArrayBuffer` — module
- `isArrayBufferView` — module
- `isAsyncFunction` — module
- `isBigInt64Array` — module
- `isBigIntObject` — module
- `isBigUint64Array` — module
- `isBooleanObject` — module
- `isBoxedPrimitive` — module
- `isCryptoKey` — module
- `isDataView` — module
- `isDate` — module
- `isExternal` — module
- `isFloat16Array` — module
- `isFloat32Array` — module
- `isFloat64Array` — module
- `isGeneratorFunction` — module
- `isGeneratorObject` — module
- `isInt16Array` — module
- `isInt32Array` — module
- `isInt8Array` — module
- `isKeyObject` — module
- `isMap` — module
- `isMapIterator` — module
- `isModuleNamespaceObject` — module
- `isNativeError` — module
- `isNumberObject` — module
- `isPromise` — module
- `isProxy` — module
- `isRegExp` — module
- `isSet` — module
- `isSetIterator` — module
- `isSharedArrayBuffer` — module
- `isStringObject` — module
- `isSymbolObject` — module
- `isTypedArray` — module
- `isUint16Array` — module
- `isUint32Array` — module
- `isUint8Array` — module
- `isUint8ClampedArray` — module
- `isWeakMap` — module
- `isWeakSet` — module

## `uuid`

### Methods

- `v1` — module
- `v3` — module
- `v4` — module
- `v5` — module
- `v7` — module
- `validate` — module
- `version` — module

## `v8`

### Classes

- `DefaultDeserializer`
- `DefaultSerializer`
- `Deserializer`
- `GCProfiler`
- `Serializer`

### Methods

- `addDeserializeCallback` — instance *(class: `startupSnapshot`)*
- `addSerializeCallback` — instance *(class: `startupSnapshot`)*
- `cachedDataVersionTag` — module
- `createHook` — instance *(class: `promiseHooks`)*
- `deserialize` — module
- `getCppHeapStatistics` — module
- `getHeapCodeStatistics` — module ⚠ **stub** — all fields 0; Perry compiles AOT, there is no JIT code heap (#4916)
- `getHeapSnapshot` — module
- `getHeapSpaceStatistics` — module ⚠ **stub** — Node space names with all live usage attributed to old_space from Perry arenas; other spaces report 0 (#4916)
- `getHeapStatistics` — module ⚠ **stub** — Node shape, Perry numbers: total_heap_size/used_heap_size/malloced_memory/total_allocated_bytes from Perry arenas, total_physical_size=RSS, heap_size_limit fixed ~2GB (not enforced); *_executable, external_memory, global-handles and zap fields are 0 (#4916)
- `isBuildingSnapshot` — instance *(class: `startupSnapshot`)*
- `isStringOneByteRepresentation` — module
- `onAfter` — instance *(class: `promiseHooks`)*
- `onBefore` — instance *(class: `promiseHooks`)*
- `onInit` — instance *(class: `promiseHooks`)*
- `onSettled` — instance *(class: `promiseHooks`)*
- `queryObjects` — module
- `readDouble` — instance *(class: `Deserializer`)*
- `readHeader` — instance *(class: `Deserializer`)*
- `readRawBytes` — instance *(class: `Deserializer`)*
- `readUint32` — instance *(class: `Deserializer`)*
- `readUint64` — instance *(class: `Deserializer`)*
- `readValue` — instance *(class: `Deserializer`)*
- `releaseBuffer` — instance *(class: `Serializer`)*
- `serialize` — module
- `setDeserializeMainFunction` — instance *(class: `startupSnapshot`)*
- `setFlagsFromString` — module
- `setHeapSnapshotNearHeapLimit` — module
- `start` — instance *(class: `GCProfiler`)*
- `startCpuProfile` — module
- `stop` — instance *(class: `GCProfiler`)* ⚠ **stub** — report has the Node shape but the statistics array is always empty (#4916)
- `stopCoverage` — module
- `takeCoverage` — module
- `writeDouble` — instance *(class: `Serializer`)*
- `writeHeader` — instance *(class: `Serializer`)*
- `writeHeapSnapshot` — module
- `writeRawBytes` — instance *(class: `Serializer`)*
- `writeUint32` — instance *(class: `Serializer`)*
- `writeUint64` — instance *(class: `Serializer`)*
- `writeValue` — instance *(class: `Serializer`)*

### Properties

- `promiseHooks`
- `startupSnapshot`

## `validator`

### Methods

- `isEmail` — module
- `isEmpty` — module
- `isJSON` — module
- `isURL` — module
- `isUUID` — module

## `vm`

### Classes

- `Script`

### Methods

- `compileFunction` — module
- `createCachedData` — instance
- `createContext` — module
- `createScript` — module
- `dependencySpecifiers` — instance
- `error` — instance
- `evaluate` — instance
- `hasAsyncGraph` — instance
- `hasTopLevelAwait` — instance
- `identifier` — instance
- `instantiate` — instance
- `isContext` — module
- `link` — instance
- `linkRequests` — instance
- `measureMemory` — module
- `moduleRequests` — instance
- `namespace` — instance
- `runInContext` — module
- `runInNewContext` — module
- `runInThisContext` — module
- `setExport` — instance
- `status` — instance

### Properties

- `constants`
- `default`

## `wasi`

### Classes

- `WASI`

### Methods

- `WASI` — module
- `finalizeBindings` — instance *(class: `WASI`)*
- `getImportObject` — instance *(class: `WASI`)*
- `initialize` — instance *(class: `WASI`)*
- `start` — instance *(class: `WASI`)*

### Properties

- `wasiImport`

## `worker_threads`

### Classes

- `BroadcastChannel`
- `MessageChannel`
- `MessagePort`
- `Worker`

### Methods

- `BroadcastChannel` — module
- `MessageChannel` — module
- `addEventListener` — instance
- `cpuUsage` — instance *(class: `Worker`)*
- `getEnvironmentData` — module
- `getHeapSnapshot` — instance *(class: `Worker`)*
- `getHeapStatistics` — instance *(class: `Worker`)*
- `isMarkedAsUntransferable` — module
- `markAsUncloneable` — module
- `markAsUntransferable` — module
- `moveMessagePortToContext` — module
- `off` — instance *(class: `Worker`)*
- `on` — instance *(class: `Worker`)*
- `once` — instance *(class: `Worker`)*
- `postMessageToThread` — module
- `receiveMessageOnPort` — module
- `ref` — instance *(class: `Worker`)*
- `removeEventListener` — instance
- `setEnvironmentData` — module
- `startCpuProfile` — instance *(class: `Worker`)*
- `startHeapProfile` — instance *(class: `Worker`)*
- `terminate` — instance *(class: `Worker`)*
- `unref` — instance *(class: `Worker`)*

### Properties

- `SHARE_ENV`
- `isInternalThread`
- `isMainThread`
- `locks`
- `parentPort`
- `resourceLimits`
- `threadId`
- `threadName`
- `workerData`

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

### Properties

- `CLOSED`
- `CLOSING`
- `CONNECTING`
- `OPEN`

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
- `createBrotliCompress` — module ⚠ **stub** — params/quality options accepted but ignored, warns once (#4917)
- `createBrotliDecompress` — module ⚠ **stub** — params/quality options accepted but ignored, warns once (#4917)
- `createDeflate` — module ⚠ **stub** — level honored; strategy/memLevel validated but not applied (#4917)
- `createDeflateRaw` — module ⚠ **stub** — level honored; strategy/memLevel validated but not applied (#4917)
- `createGunzip` — module
- `createGzip` — module ⚠ **stub** — level honored; strategy/memLevel validated but not applied (#4917)
- `createInflate` — module
- `createInflateRaw` — module
- `createUnzip` — module
- `createZstdCompress` — module ⚠ **stub** — params/quality options accepted but ignored, warns once (#4917)
- `createZstdDecompress` — module ⚠ **stub** — params/quality options accepted but ignored, warns once (#4917)
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
- `zstdCompress` — module
- `zstdCompressSync` — module
- `zstdDecompress` — module
- `zstdDecompressSync` — module

### Properties

- `codes`
- `constants`
