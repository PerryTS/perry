# Perry Runtime Parity Gap List

This document is a structured gap analysis comparing the public Node.js + Bun runtime API surface (catalogued in `runtime-parity.md`) against the APIs Perry can dispatch at compile time. Coverage sources are: the unimplemented-API gate manifest (`crates/perry-api-manifest/src/entries.rs`, ~590 (module, method) entries), `Expr::*` HIR variants in `crates/perry-hir/src/ir.rs` that lower stdlib APIs directly to dedicated codegen arms (~301 stdlib-shaped variants), and `js_*` FFI exports across `perry-runtime` (~984), `perry-stdlib` (~633), and 35 `perry-ext-*` crates (~553). Output is intended for prioritizing which APIs to implement next.

## Summary

| Category | Modules | Gap APIs | Verified-covered |
|----------|---------|----------|------------------|
| Whole-module gaps (zero coverage) | 16 | 427 | n/a |
| Partial-module gaps | 31 | 1656 | 406 |
| Web-global gaps | — | 282 | 107 |
| Bun-only gaps (out of scope) | — | 394 | n/a |
| **Total true gaps** |  | **2365** |  |

**Top modules by remaining true gaps (Node + Web):**

- `Web / Global APIs` — 282
- `node:os` — 195
- `node:fs` — 133
- `node:crypto` — 128
- `node:process (and global `process`)` — 99
- `node:util` — 92
- `node:http2` — 97
- `node:test (and node:test/reporters, node:test/mock)` — 93
- `node:http` — 89
- `node:zlib` — 79
- `node:stream` — 76
- `node:worker_threads` — 60

## Whole-module gaps

Modules with **zero** Perry coverage across the manifest, `Expr::*` variants, or FFI exports. Every parity-reference API in these modules is a gap. Listed in descending API-count order.

### node:test (and node:test/reporters, node:test/mock)

**Total APIs: 93** · Perry covers: 0 · Gap: 93

Selected highlights (full list in `runtime-parity.md`):

- `test([name][, options][, fn])`
- `test.skip([name][, options][, fn])`
- `test.todo([name][, options][, fn])`
- `test.only([name][, options][, fn])`
- `suite([name][, options][, fn])`
- `suite.skip([name][, options][, fn])`
- `suite.todo([name][, options][, fn])`
- `suite.only([name][, options][, fn])`
- `describe([name][, options][, fn])`
- `it([name][, options][, fn])`
- `before([fn][, options])`
- `after([fn][, options])`
- … and 81 more

### node:v8

**Total APIs: 58** · Perry covers: 0 · Gap: 58

Selected highlights (full list in `runtime-parity.md`):

- `v8.cachedDataVersionTag()`
- `v8.getHeapCodeStatistics()`
- `v8.getHeapSnapshot([options])`
- `v8.getHeapSpaceStatistics()`
- `v8.getHeapStatistics()`
- `v8.getCppHeapStatistics([detailLevel])`
- `v8.queryObjects(ctor[, options])`
- `v8.setFlagsFromString(flags)`
- `v8.stopCoverage()`
- `v8.takeCoverage()`
- `v8.writeHeapSnapshot([filename[, options]])`
- `v8.setHeapSnapshotNearHeapLimit(limit)`
- … and 46 more

### node:dns

**Total APIs: 53** · Perry covers: 0 · Gap: 53

Selected highlights (full list in `runtime-parity.md`):

- `dns.lookup(hostname[, options], callback)`
- `dns.lookupService(address, port, callback)`
- `dns.resolve(hostname[, rrtype], callback)`
- `dns.resolve4(hostname[, options], callback)`
- `dns.resolve6(hostname[, options], callback)`
- `dns.resolveAny(hostname, callback)`
- `dns.resolveCaa(hostname, callback)`
- `dns.resolveCname(hostname, callback)`
- `dns.resolveMx(hostname, callback)`
- `dns.resolveNaptr(hostname, callback)`
- `dns.resolveNs(hostname, callback)`
- `dns.resolvePtr(hostname, callback)`
- … and 41 more

### node:cluster

**Total APIs: 35** · Perry covers: primary lifecycle subset · Gap: worker handle distribution and remaining events

Selected highlights (full list in `runtime-parity.md`):

- `cluster.isPrimary`
- `cluster.isMaster`
- `cluster.isWorker`
- `cluster.worker`
- `cluster.workers`
- `cluster.settings`
- `cluster.schedulingPolicy`
- `cluster.SCHED_RR`
- `cluster.SCHED_NONE`
- `cluster.fork([env])`
- `cluster.disconnect([callback])`
- `cluster.setupPrimary([settings])`
- `cluster.setupMaster([settings])`
- worker handle identity and disconnect lifecycle
- … and remaining Worker/listening events

### node:vm

**Total APIs: 32** · Perry covers: 0 · Gap: 32

Selected highlights (full list in `runtime-parity.md`):

- `new vm.Script(code[, options])`
- `script.cachedDataRejected`
- `script.sourceMapURL`
- `script.createCachedData()`
- `script.runInContext(contextifiedObject[, options])`
- `script.runInNewContext([contextObject[, options]])`
- `script.runInThisContext([options])`
- `module.error`
- `module.identifier`
- `module.namespace`
- `module.status`
- `module.evaluate([options])`
- … and 20 more

### node:diagnostics_channel

**Total APIs: 30** · Perry covers: 0 · Gap: 30

Selected highlights (full list in `runtime-parity.md`):

- `diagnostics_channel.hasSubscribers(name)`
- `diagnostics_channel.channel(name)`
- `diagnostics_channel.subscribe(name, onMessage)`
- `diagnostics_channel.unsubscribe(name, onMessage)`
- `diagnostics_channel.tracingChannel(nameOrChannels)`
- `diagnostics_channel.boundedChannel(nameOrChannels)`
- `channel.hasSubscribers`
- `channel.publish(message)`
- `channel.subscribe(onMessage)`
- `channel.unsubscribe(onMessage)`
- `channel.bindStore(store[, transform])`
- `channel.unbindStore(store)`
- … and 18 more

### node:dgram

**Total APIs: 28** · Perry covers: 27 · Gap: 1

Selected highlights (full list in `runtime-parity.md`):

- Deterministic loopback coverage: `createSocket`, `bind`, `address`, `send`, `message`, `connect`, `remoteAddress`, `disconnect`, `close`, `ref` / `unref`, buffer-size and send-queue getters, socket option validation/returns, and multicast/source-specific membership validation/returns.
- Remaining gaps: real host UDP IO, OS-level multicast membership side effects, host socket option side effects, and `socket[Symbol.asyncDispose]()`.
- `socket[Symbol.asyncDispose]()`

### node:dns/promises

**Total APIs: 21** · Perry covers: 0 · Gap: 21

Selected highlights (full list in `runtime-parity.md`):

- `dnsPromises.lookup(hostname[, options])`
- `dnsPromises.lookupService(address, port)`
- `dnsPromises.resolve(hostname[, rrtype])`
- `dnsPromises.resolve4(hostname[, options])`
- `dnsPromises.resolve6(hostname[, options])`
- `dnsPromises.resolveAny(hostname)`
- `dnsPromises.resolveCaa(hostname)`
- `dnsPromises.resolveCname(hostname)`
- `dnsPromises.resolveMx(hostname)`
- `dnsPromises.resolveNaptr(hostname)`
- `dnsPromises.resolveNs(hostname)`
- `dnsPromises.resolvePtr(hostname)`
- … and 9 more

### node:inspector (and node:inspector/promises)

**Total APIs: 19** · Perry covers: 0 · Gap: 19

Selected highlights (full list in `runtime-parity.md`):

- `inspector.open([port[, host[, wait]]])`
- `inspector.close()`
- `inspector.url()`
- `inspector.waitForDebugger()`
- `inspector.console`
- `new Session()`
- `session.connect()`
- `session.connectToMainThread()`
- `session.disconnect()`
- `'inspectorNotification'`
- `inspector.Network.requestWillBeSent(params)`
- `inspector.Network.responseReceived(params)`
- … and 7 more

### node:repl

**Total APIs: 17** · Perry covers: 0 · Gap: 17

Selected highlights (full list in `runtime-parity.md`):

- `repl.start([options])`
- `repl.builtinModules`
- `repl.REPL_MODE_SLOPPY`
- `repl.REPL_MODE_STRICT`
- `repl.Recoverable`
- `replServer.context`
- `replServer.editorMode`
- `replServer.useColors`
- `replServer.useGlobal`
- `replServer.ignoreUndefined`
- `replServer.replMode`
- `replServer.defineCommand(keyword, cmd)`
- … and 5 more

### node:domain

**Total APIs: 10** · Perry covers: 0 · Gap: 10

Selected highlights (full list in `runtime-parity.md`):

- `domain.create()`
- `domain.active`
- `domain.members`
- `domain.add(emitter)`
- `domain.bind(callback)`
- `domain.intercept(callback)`
- `domain.enter()`
- `domain.exit()`
- `domain.remove(emitter)`
- `domain.run(fn[, ...args])`

### node:readline/promises

**Total APIs: 7** · Perry covers: 0 · Gap: 7

Selected highlights (full list in `runtime-parity.md`):

- `readlinePromises.createInterface(options)`
- `rl.clearLine(dir)`
- `rl.clearScreenDown()`
- `rl.cursorTo(x[, y])`
- `rl.moveCursor(dx, dy)`
- `rl.commit()`
- `rl.rollback()`

### node:stream/consumers

**Total APIs: 6** · Perry covers: 0 · Gap: 6

Selected highlights (full list in `runtime-parity.md`):

- `consumers.arrayBuffer(stream)`
- `consumers.blob(stream)`
- `consumers.buffer(stream)`
- `consumers.bytes(stream)`
- `consumers.json(stream)`
- `consumers.text(stream)`

### node:string_decoder

**Total APIs: 6** · Perry covers: 0 · Gap: 6

Selected highlights (full list in `runtime-parity.md`):

- `new StringDecoder([encoding])`
- `stringDecoder.write(buffer)`
- `stringDecoder.end([buffer])`
- `stringDecoder.lastChar`
- `stringDecoder.lastNeed`
- `stringDecoder.lastTotal`

### node:timers/promises

**Total APIs: 5** · Perry covers: 0 · Gap: 5

Selected highlights (full list in `runtime-parity.md`):

- `setTimeout([delay[, value[, options]]])`
- `setImmediate([value[, options]])`
- `setInterval([delay[, value[, options]]])`
- `scheduler.wait(delay[, options])`
- `scheduler.yield()`

## Partial-module gaps

Modules where Perry has at least one coverage source. Listed in descending gap-size order.

### node:os

**Gap APIs: 195** · Already covered: 14

#### Missing from Perry

- `os.availableParallelism()`
- `os.endianness()`
- `os.getPriority([pid])`
- `os.loadavg()`
- `os.machine()`
- `os.setPriority([pid, ]priority)`
- `os.version()`
- `os.devNull`
- `os.constants`
- `SIGHUP`
- `SIGINT`
- `SIGQUIT`
- `SIGILL`
- `SIGTRAP`
- `SIGABRT`
- `SIGIOT`
- `SIGBUS`
- `SIGFPE`
- `SIGKILL`
- `SIGUSR1`
- `SIGUSR2`
- `SIGSEGV`
- `SIGPIPE`
- `SIGALRM`
- `SIGTERM`
- `SIGCHLD`
- `SIGSTKFLT`
- `SIGCONT`
- `SIGSTOP`
- `SIGTSTP`
- `SIGBREAK`
- `SIGTTIN`
- `SIGTTOU`
- `SIGURG`
- `SIGXCPU`
- `SIGXFSZ`
- `SIGVTALRM`
- `SIGPROF`
- `SIGWINCH`
- `SIGIO`
- `SIGPOLL`
- `SIGLOST`
- `SIGPWR`
- `SIGINFO`
- `SIGSYS`
- `SIGUNUSED`
- `E2BIG`
- `EACCES`
- `EADDRINUSE`
- `EADDRNOTAVAIL`
- … and 145 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `os.arch()` | `manifest:os.arch` |
| `os.cpus()` | `manifest:os.cpus` |
| `os.freemem()` | `manifest:os.freemem` |
| `os.homedir()` | `manifest:os.homedir` |
| `os.hostname()` | `manifest:os.hostname` |
| `os.networkInterfaces()` | `manifest:os.networkInterfaces` |
| `os.platform()` | `manifest:os.platform` |
| `os.release()` | `manifest:os.release` |
| `os.tmpdir()` | `manifest:os.tmpdir` |
| `os.totalmem()` | `manifest:os.totalmem` |
| `os.type()` | `manifest:os.type` |
| `os.uptime()` | `manifest:os.uptime` |
| `os.userInfo([options])` | `manifest:os.userInfo` |
| `os.EOL` | `expr:OsEOL` |

### node:fs

**Gap APIs: 133** · Already covered: 46

#### Missing from Perry

- `fs.chown(path, uid, gid, callback)`
- `fs.close(fd[, callback])`
- `fs.cp(src, dest[, options], callback)`
- `fs.fchmod(fd, mode, callback)`
- `fs.fchown(fd, uid, gid, callback)`
- `fs.fdatasync(fd, callback)`
- `fs.fstat(fd[, options], callback)`
- `fs.fsync(fd, callback)`
- `fs.ftruncate(fd[, len], callback)`
- `fs.futimes(fd, atime, mtime, callback)`
- `fs.glob(pattern[, options], callback)`
- `fs.lchmod(path, mode, callback)`
- `fs.lchown(path, uid, gid, callback)`
- `fs.lstat(path[, options], callback)`
- `fs.lutimes(path, atime, mtime, callback)`
- `fs.open(path[, flags[, mode]], callback)`
- `fs.openAsBlob(path[, options])`
- `fs.opendir(path[, options], callback)`
- `fs.read(fd, buffer, offset, length, position, callback)`
- `fs.read(fd[, options], callback)`
- `fs.read(fd, buffer[, options], callback)`
- `fs.readv(fd, buffers[, position], callback)`
- `fs.realpath.native(path[, options], callback)`
- `fs.statfs(path[, options], callback)`
- `fs.truncate(path[, len], callback)`
- `fs.utimes(path, atime, mtime, callback)`
- `fs.watch(filename[, options][, listener])`
- `fs.write(fd, buffer, offset[, length[, position]], callback)`
- `fs.write(fd, buffer[, options], callback)`
- `fs.write(fd, string[, position[, encoding]], callback)`
- `fs.writev(fd, buffers[, position], callback)`
- `fs.chownSync(path, uid, gid)`
- `fs.closeSync(fd)`
- `fs.cpSync(src, dest[, options])`
- `fs.fchmodSync(fd, mode)`
- `fs.fchownSync(fd, uid, gid)`
- `fs.fdatasyncSync(fd)`
- `fs.fstatSync(fd[, options])`
- `fs.fsyncSync(fd)`
- `fs.ftruncateSync(fd[, len])`
- `fs.futimesSync(fd, atime, mtime)`
- `fs.globSync(pattern[, options])`
- `fs.lchmodSync(path, mode)`
- `fs.lchownSync(path, uid, gid)`
- `fs.lutimesSync(path, atime, mtime)`
- `fs.mkdtempDisposableSync(prefix[, options])`
- … and 87 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `fs.access(path[, mode], callback)` | `ffi:js_fs_access_sync` |
| `fs.appendFile(path, data[, options], callback)` | `manifest:fs.appendFile` |
| `fs.chmod(path, mode, callback)` | `ffi:js_fs_chmod_sync` |
| `fs.copyFile(src, dest[, mode], callback)` | `ffi:js_fs_copy_file_sync` |
| `fs.createReadStream(path[, options])` | `manifest:fs.createReadStream` |
| `fs.createWriteStream(path[, options])` | `manifest:fs.createWriteStream` |
| `fs.exists(path, callback)` | `ffi:js_fs_exists_sync` |
| `fs.link(existingPath, newPath, callback)` | `ffi:js_fs_link_sync` |
| `fs.linkSync(existingPath, newPath)` | `ffi:js_fs_link_sync` |
| `fs.mkdir(path[, options], callback)` | `manifest:fs.mkdir` |
| `fs.mkdtemp(prefix[, options], callback)` | `ffi:js_fs_mkdtemp_sync` |
| `fs.readdir(path[, options], callback)` | `manifest:fs.readdir` |
| `fs.readFile(path[, options], callback)` | `manifest:fs.readFile` |
| `fs.readlink(path[, options], callback)` | `ffi:js_fs_readlink_dispatch` |
| `fs.readlinkSync(path[, options])` | `ffi:js_fs_readlink_dispatch` |
| `fs.realpath(path[, options], callback)` | `ffi:js_fs_realpath_sync` |
| `fs.rename(oldPath, newPath, callback)` | `ffi:js_fs_rename_sync` |
| `fs.rm(path[, options], callback)` | `manifest:fs.rm` |
| `fs.rmdir(path[, options], callback)` | `ffi:js_fs_rmdir_sync` |
| `fs.symlink(target, path[, type], callback)` | `ffi:js_fs_symlink_sync` |
| `fs.symlinkSync(target, path[, type])` | `ffi:js_fs_symlink_sync` |
| … | 25 more covered APIs |

### node:crypto

**Gap APIs: 128** · Already covered: 10

#### Missing from Perry

- `crypto.checkPrime(candidate[, options], callback)`
- `crypto.checkPrimeSync(candidate[, options])`
- `crypto.createCipheriv(algorithm, key, iv[, options])`
- `crypto.createDecipheriv(algorithm, key, iv[, options])`
- `crypto.createDiffieHellman(prime[, primeEncoding][, generator][, generatorEncoding])`
- `crypto.createDiffieHellman(primeLength[, generator])`
- `crypto.createDiffieHellmanGroup(name)`
- `crypto.createECDH(curveName)`
- `crypto.createPrivateKey(key)`
- `crypto.createPublicKey(key)`
- `crypto.createSecretKey(key[, encoding])`
- `crypto.createSign(algorithm[, options])`
- `crypto.createVerify(algorithm[, options])`
- `crypto.diffieHellman(options[, callback])`
- `crypto.generateKey(type, options, callback)`
- `crypto.generateKeySync(type, options)`
- `crypto.generateKeyPair(type, options, callback)`
- `crypto.generateKeyPairSync(type, options)`
- `crypto.generatePrime(size[, options], callback)`
- `crypto.generatePrimeSync(size[, options])`
- `crypto.getCipherInfo(nameOrNid[, options])`
- `crypto.getCiphers()`
- `crypto.getCurves()`
- `crypto.getDiffieHellman(groupName)`
- `crypto.getFips()`
- `crypto.getHashes()`
- `crypto.hash(algorithm, data[, options])`
- `crypto.hkdf(digest, ikm, salt, info, keylen, callback)`
- `crypto.hkdfSync(digest, ikm, salt, info, keylen)`
- `crypto.privateDecrypt(privateKey, buffer)`
- `crypto.privateEncrypt(privateKey, buffer)`
- `crypto.publicDecrypt(key, buffer)`
- `crypto.publicEncrypt(key, buffer)`
- `crypto.randomFill(buffer[, offset][, size], callback)`
- `crypto.randomFillSync(buffer[, offset][, size])`
- `crypto.randomInt([min, ]max[, callback])`
- `crypto.randomUUIDv7([options])`
- `crypto.scryptSync(password, salt, keylen[, options])`
- `crypto.secureHeapUsed()`
- `crypto.setEngine(engine[, flags])`
- `crypto.setFips(bool)`
- `crypto.sign(algorithm, data, key[, callback])`
- `crypto.timingSafeEqual(a, b)`
- `crypto.verify(algorithm, data, key, signature[, callback])`
- `crypto.argon2(algorithm, parameters, callback)`
- `crypto.argon2Sync(algorithm, parameters)`
- `crypto.encapsulate(key[, callback])`
- `crypto.decapsulate(key, ciphertext[, callback])`
- `crypto.constants`
- `crypto.fips`
- … and 78 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `crypto.createHash(algorithm[, options])` | `manifest:crypto.createHash` |
| `crypto.createHmac(algorithm, key[, options])` | `manifest:crypto.createHmac` |
| `crypto.getRandomValues(typedArray)` | `manifest:crypto.getRandomValues` |
| `crypto.pbkdf2(password, salt, iterations, keylen, digest, callback)` | `manifest:crypto.pbkdf2` |
| `crypto.pbkdf2Sync(password, salt, iterations, keylen, digest)` | `manifest:crypto.pbkdf2Sync` |
| `crypto.randomBytes(size[, callback])` | `manifest:crypto.randomBytes` |
| `crypto.randomUUID([options])` | `manifest:crypto.randomUUID` |
| `crypto.scrypt(password, salt, keylen[, options], callback)` | `ffi:js_crypto_scrypt` |
| `crypto.webcrypto.getRandomValues(typedArray)` | `manifest:crypto.getRandomValues` |
| `crypto.webcrypto.randomUUID()` | `manifest:crypto.randomUUID` |

### node:process (and global `process`)

**Gap APIs: 97** · Already covered: 21

#### Missing from Perry

- `process.abort()`
- `process.execve(file[, args[, env]])`
- `process.memoryUsage.rss()`
- `process.availableMemory()`
- `process.constrainedMemory()`
- `process.resourceUsage()`
- `process.getActiveResourcesInfo()`
- `process.getuid()`
- `process.geteuid()`
- `process.setuid(id)`
- `process.seteuid(id)`
- `process.getgid()`
- `process.getegid()`
- `process.setgid(id)`
- `process.setegid(id)`
- `process.getgroups()`
- `process.setgroups(groups)`
- `process.initgroups(user, extraGroup)`
- `process.send(message[, sendHandle[, options]][, callback])`
- `process.disconnect()`
- `process.channel`
- `process.channel.ref()`
- `process.channel.unref()`
- `process.emitWarning(warning[, options])`
- `process.setUncaughtExceptionCaptureCallback(fn)`
- `process.addUncaughtExceptionCaptureCallback(fn)`
- `process.hasUncaughtExceptionCaptureCallback()`
- `process.dlopen(module, filename[, flags])`
- `process.loadEnvFile(path)`
- `process.hrtime([time])`
- `process.permission.has(scope[, reference])`
- `process.umask()`
- `process.umask(mask)`
- `process.finalization.register(ref, callback)`
- `process.finalization.registerBeforeExit(ref, callback)`
- `process.finalization.unregister(ref)`
- `process.ref(maybeRefable)`
- `process.unref(maybeRefable)`
- `process.report.getReport([err])`
- `process.report.writeReport([filename][, err])`
- `process.binding(name)`
- `process.platform`
- `process.arch`
- `process.release`
- `process.argv0`
- `process.execPath`
- … and 51 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `process.exit([code])` | `expr:ProcessExit` |
| `process.chdir(directory)` | `expr:ProcessChdir` |
| `process.cwd()` | `expr:ProcessCwd` |
| `process.memoryUsage()` | `expr:ProcessMemoryUsage` |
| `process.cpuUsage([previousValue])` | `expr:ProcessCpuUsage` |
| `process.threadCpuUsage([previousValue])` | `expr:ProcessThreadCpuUsage` |
| `process.uptime()` | `expr:ProcessUptime` |
| `process.kill(pid[, signal])` | `expr:ProcessKill` |
| `process.hrtime.bigint()` | `expr:ProcessHrtimeBigint` |
| `process.nextTick(callback[, ...args])` | `expr:ProcessNextTick` |
| `process.pid` | `expr:ProcessPid` |
| `process.ppid` | `expr:ProcessPpid` |
| `process.version` | `expr:ProcessVersion` |
| `process.versions` | `expr:ProcessVersions` |
| `process.argv` | `expr:ProcessArgv` |
| `process.env` | `expr:ProcessEnv` |
| `process.stdin` | `expr:ProcessStdin` |
| `process.sourceMapsEnabled` | `manifest:process.sourceMapsEnabled` |
| `process.setSourceMapsEnabled(val)` | `manifest:process.setSourceMapsEnabled` |
| … | 2 more covered APIs |

### node:util

**Gap APIs: 89** · Already covered: 13

#### Missing from Perry

- `util.debuglog(section[, callback])`
- `util.debug(section)`
- `util.formatWithOptions(inspectOptions, format[, ...args])`
- `util.parseEnv(content)`
- `util.stripVTControlCharacters(str)`
- `util.toUSVString(string)`
- `util.setTraceSigInt(enable)`
- `MIMEType.prototype.type`
- `MIMEType.prototype.subtype`
- `MIMEType.prototype.essence`
- `MIMEType.prototype.params`
- `MIMEType.prototype.toString()`
- `MIMEType.prototype.toJSON()`
- `MIMEParams.prototype.delete(name)`
- `MIMEParams.prototype.entries()`
- `MIMEParams.prototype.get(name)`
- `MIMEParams.prototype.has(name)`
- `MIMEParams.prototype.keys()`
- `MIMEParams.prototype.set(name, value)`
- `MIMEParams.prototype.values()`
- `util.inspect.custom`
- `util.inspect.defaultOptions`
- `util.inspect.styles`
- `util.inspect.colors`
- `util.promisify.custom`
- `util.types.isAnyArrayBuffer(value)`
- `util.types.isArrayBuffer(value)`
- `util.types.isArrayBufferView(value)`
- `util.types.isArgumentsObject(value)`
- `util.types.isAsyncFunction(value)`
- `util.types.isBigInt64Array(value)`
- `util.types.isBigUint64Array(value)`
- `util.types.isBooleanObject(value)`
- `util.types.isBoxedPrimitive(value)`
- `util.types.isCryptoKey(value)`
- `util.types.isDataView(value)`
- `util.types.isDate(value)`
- `util.types.isExternal(value)`
- `util.types.isFloat16Array(value)`
- … and 48 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `util.callbackify(original)` | `manifest:util.callbackify` |
| `util.deprecate(fn, msg[, code[, options]])` | `manifest:util.deprecate` |
| `util.format(format[, ...args])` | `manifest:util.format` |
| `util.getCallSites([frameCount][, options])` | `manifest:util.getCallSites` |
| `util.getSystemErrorName(err)` | `manifest:util.getSystemErrorName` |
| `util.getSystemErrorMap()` | `manifest:util.getSystemErrorMap` |
| `util.getSystemErrorMessage(err)` | `manifest:util.getSystemErrorMessage` |
| `util.inherits(constructor, superConstructor)` | `manifest:util.inherits` |
| `util.inspect(object[, options])` | `manifest:util.inspect` |
| `util.isDeepStrictEqual(val1, val2[, options])` | `manifest:util.isDeepStrictEqual` |
| `util.promisify(original)` | `manifest:util.promisify` |

### node:http2

**Gap APIs: 97** · Already covered: 5

#### Missing from Perry

- `http2.createServer([options][, onRequestHandler])`
- `http2.connect(authority[, options][, listener])`
- `http2.getDefaultSettings()`
- `http2.getPackedSettings([settings])`
- `http2.getUnpackedSettings(buf)`
- `http2.performServerHandshake(socket[, options])`
- `http2.constants`
- `session.alpnProtocol`
- `session.closed`
- `session.connecting`
- `session.destroy([error][, code])`
- `session.destroyed`
- `session.encrypted`
- `session.goaway([code[, lastStreamID[, opaqueData]]])`
- `session.localSettings`
- `session.originSet`
- `session.pendingSettingsAck`
- `session.ping([payload, ]callback)`
- `session.ref()`
- `session.remoteSettings`
- `session.setLocalWindowSize(windowSize)`
- `session.setTimeout(msecs, callback)`
- `session.socket`
- `session.state`
- `session.settings([settings][, callback])`
- `session.type`
- `session.unref()`
- `serverSession.altsvc(alt, originOrStream)`
- `serverSession.origin(...origins)`
- `clientSession.request(headers[, options])`
- `stream.aborted`
- `stream.bufferSize`
- `stream.closed`
- `stream.destroyed`
- `stream.endAfterHeaders`
- `stream.id`
- `stream.pending`
- `stream.priority(options)`
- `stream.rstCode`
- `stream.sentHeaders`
- `stream.sentInfoHeaders`
- `stream.sentTrailers`
- `stream.session`
- `stream.setTimeout(msecs, callback)`
- `stream.state`
- `stream.sendTrailers(headers)`
- `serverStream.additionalHeaders(headers)`
- `serverStream.headersSent`
- `serverStream.pushAllowed`
- `serverStream.pushStream(headers[, options], callback)`
- … and 47 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `http2.createSecureServer(options[, onRequestHandler])` | `manifest:http2.createSecureServer` |
| `session.close([callback])` | `manifest:http2.close` |
| `stream.close(code[, callback])` | `manifest:http2.close` |
| `http2Server.close([callback])` | `manifest:http2.close` |
| `response.getHeaders()` | `ffi:js_response_get_headers` |

### node:http

**Gap APIs: 89** · Already covered: 52

#### Missing from Perry

- `http.validateHeaderName(name[, label])`
- `http.validateHeaderValue(name, value)`
- `http.setMaxIdleHTTPParsers(max)`
- `http.setGlobalProxyFromEnv([proxyEnv])`
- `http.METHODS`
- `http.STATUS_CODES`
- `http.globalAgent`
- `http.maxHeaderSize`
- `new Agent([options])`
- `agent.createConnection(options[, callback])`
- `agent.keepSocketAlive(socket)`
- `agent.reuseSocket(socket, request)`
- `agent.getName([options])`
- `agent.freeSockets`
- `agent.maxFreeSockets`
- `agent.maxSockets`
- `agent.maxTotalSockets`
- `agent.requests`
- `agent.sockets`
- `request.abort()`
- `request.cork()`
- `request.getHeaderNames()`
- `request.getHeaders()`
- `request.getRawHeaderNames()`
- `request.setNoDelay([noDelay])`
- `request.setSocketKeepAlive([enable][, initialDelay])`
- `request.uncork()`
- `request.aborted`
- `request.connection`
- `request.destroyed`
- `request.finished`
- `request.host`
- `request.maxHeadersCount`
- `request.protocol`
- `request.reusedSocket`
- `request.socket`
- `request.writableEnded`
- `request.writableFinished`
- `server[Symbol.asyncDispose]()`
- `server.headersTimeout`
- `server.keepAliveTimeout`
- `server.keepAliveTimeoutBuffer`
- `server.listening`
- `server.maxHeadersCount`
- `server.maxRequestsPerSocket`
- `server.requestTimeout`
- `server.timeout`
- `response.addTrailers(headers)`
- `response.cork()`
- `response.getHeaderNames()`
- … and 39 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `http.createServer([options][, requestListener])` | `manifest:http.createServer` |
| `http.get(options[, callback])` | `manifest:http.get` |
| `http.get(url[, options][, callback])` | `manifest:http.get` |
| `http.request(options[, callback])` | `manifest:http.request` |
| `http.request(url[, options][, callback])` | `manifest:http.request` |
| `agent.destroy()` | `manifest:http.destroy` |
| `request.end([data[, encoding]][, callback])` | `manifest:http.end` |
| `request.destroy([error])` | `manifest:http.destroy` |
| `request.flushHeaders()` | `manifest:http.flushHeaders` |
| `request.getHeader(name)` | `manifest:http.getHeader` |
| `request.hasHeader(name)` | `manifest:http.hasHeader` |
| `request.removeHeader(name)` | `manifest:http.removeHeader` |
| `request.setHeader(name, value)` | `manifest:http.setHeader` |
| `request.setTimeout(timeout[, callback])` | `ffi:js_http_set_timeout` |
| `request.write(chunk[, encoding][, callback])` | `manifest:http.write` |
| … | 37 more covered APIs |

### node:zlib

**Gap APIs: 79** · Already covered: 12

#### Missing from Perry

- `zlib.DeflateRaw`
- `zlib.InflateRaw`
- `zlib.Unzip`
- `zlib.BrotliCompress`
- `zlib.BrotliDecompress`
- `zlib.ZstdCompress`
- `zlib.ZstdDecompress`
- `zlib.createDeflate([options])`
- `zlib.createDeflateRaw([options])`
- `zlib.createGunzip([options])`
- `zlib.createGzip([options])`
- `zlib.createInflate([options])`
- `zlib.createInflateRaw([options])`
- `zlib.createUnzip([options])`
- `zlib.createBrotliCompress([options])`
- `zlib.createBrotliDecompress([options])`
- `zlib.createZstdCompress([options])`
- `zlib.createZstdDecompress([options])`
- `zlib.deflateRaw(buffer[, options], callback)`
- `zlib.deflateRawSync(buffer[, options])`
- `zlib.inflateRaw(buffer[, options], callback)`
- `zlib.inflateRawSync(buffer[, options])`
- `zlib.unzip(buffer[, options], callback)`
- `zlib.unzipSync(buffer[, options])`
- `zlib.brotliCompress(buffer[, options], callback)`
- `zlib.brotliCompressSync(buffer[, options])`
- `zlib.brotliDecompress(buffer[, options], callback)`
- `zlib.brotliDecompressSync(buffer[, options])`
- `zlib.zstdCompress(buffer[, options], callback)`
- `zlib.zstdCompressSync(buffer[, options])`
- `zlib.zstdDecompress(buffer[, options], callback)`
- `zlib.zstdDecompressSync(buffer[, options])`
- `zlib.close([callback])`
- `zlib.flush([kind,] callback)`
- `zlib.params(level, strategy, callback)`
- `zlib.reset()`
- `zlib.bytesWritten`
- `zlib.bytesRead`
- `zlib.crc32(data[, value])`
- `zlib.constants`
- `Z_NO_FLUSH`
- `Z_PARTIAL_FLUSH`
- `Z_SYNC_FLUSH`
- `Z_FULL_FLUSH`
- `Z_FINISH`
- `Z_BLOCK`
- `Z_TREES`
- `Z_OK`
- `Z_STREAM_END`
- `Z_NEED_DICT`
- … and 29 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `zlib.Deflate` | `ffi:js_zlib_deflate` |
| `zlib.Gzip` | `ffi:js_zlib_gzip` |
| `zlib.Gunzip` | `ffi:js_zlib_gunzip` |
| `zlib.Inflate` | `ffi:js_zlib_inflate` |
| `zlib.deflate(buffer[, options], callback)` | `ffi:js_zlib_deflate` |
| `zlib.deflateSync(buffer[, options])` | `ffi:js_zlib_deflate_sync` |
| `zlib.gzip(buffer[, options], callback)` | `manifest:zlib.gzip` |
| `zlib.gzipSync(buffer[, options])` | `ffi:js_zlib_gzip_sync` |
| `zlib.gunzip(buffer[, options], callback)` | `manifest:zlib.gunzip` |
| `zlib.gunzipSync(buffer[, options])` | `ffi:js_zlib_gunzip_sync` |
| `zlib.inflate(buffer[, options], callback)` | `ffi:js_zlib_inflate` |
| `zlib.inflateSync(buffer[, options])` | `ffi:js_zlib_inflate_sync` |

### node:stream

**Gap APIs: 76** · Already covered: 5

#### Missing from Perry

- `stream.compose(...streams)`
- `stream.isReadable(stream)`
- `stream.isWritable(stream)`
- `stream.isErrored(stream)`
- `stream.getDefaultHighWaterMark(objectMode)`
- `stream.setDefaultHighWaterMark(objectMode, value)`
- `stream.addAbortSignal(signal, stream)`
- `stream.duplexPair([options])`
- `stream.Readable.fromWeb(readableStream[, options])`
- `stream.Readable.toWeb(streamReadable[, options])`
- `stream.Readable.isDisturbed(stream)`
- `stream.Writable.fromWeb(writableStream[, options])`
- `stream.Writable.toWeb(streamWritable)`
- `stream.Duplex.fromWeb(pair[, options])`
- `stream.Duplex.toWeb(streamDuplex[, options])`
- `readable.read([size])`
- `readable.pause()`
- `readable.resume()`
- `readable.pipe(destination[, options])`
- `readable.unpipe([destination])`
- `readable.unshift(chunk[, encoding])`
- `readable.setEncoding(encoding)`
- `readable.isPaused()`
- `readable.destroy([error])`
- `readable.wrap(stream)`
- `readable.compose(stream[, options])`
- `readable.iterator([options])`
- `readable.map(fn[, options])`
- `readable.filter(fn[, options])`
- `readable.forEach(fn[, options])`
- `readable.toArray([options])`
- `readable.some(fn[, options])`
- `readable.find(fn[, options])`
- `readable.every(fn[, options])`
- `readable.flatMap(fn[, options])`
- `readable.drop(limit[, options])`
- `readable.take(limit[, options])`
- `readable.reduce(fn[, initial[, options]])`
- `readable.push(chunk[, encoding])`
- `readable.readable`
- `readable.readableFlowing`
- `readable.readableLength`
- `readable.readableHighWaterMark`
- `readable.readableAborted`
- `readable.readableDidRead`
- `readable.readableEncoding`
- `readable.readableEnded`
- `readable.readableObjectMode`
- `readable.errored`
- `readable.destroyed`
- … and 26 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `stream.pipeline(source[, ...transforms], destination, callback)` | `manifest:stream.pipeline` |
| `stream.pipeline(streams, callback)` | `manifest:stream.pipeline` |
| `stream.finished(stream[, options], callback)` | `manifest:stream.finished` |
| `stream.Readable.from(iterable[, options])` | `manifest:stream.from` |
| `stream.Duplex.from(src)` | `manifest:stream.from` |

### node:worker_threads

**Gap APIs: 60** · Already covered: 4

#### Missing from Perry

- `worker_threads.getEnvironmentData(key)`
- `worker_threads.setEnvironmentData(key[, value])`
- `worker_threads.markAsUntransferable(object)`
- `worker_threads.isMarkedAsUntransferable(object)`
- `worker_threads.markAsUncloneable(object)`
- `worker_threads.moveMessagePortToContext(port, ctx)`
- `worker_threads.receiveMessageOnPort(port)`
- `worker_threads.postMessageToThread(threadId, value[, transferList][, timeout])`
- `worker_threads.isMainThread`
- `worker_threads.isInternalThread`
- `worker_threads.threadId`
- `worker_threads.threadName`
- `worker_threads.workerData`
- `worker_threads.resourceLimits`
- `worker_threads.SHARE_ENV`
- `worker_threads.locks`
- `new Worker(filename[, options])`
- `worker.getHeapSnapshot([options])`
- `worker.getHeapStatistics()`
- `worker.cpuUsage([prev])`
- `worker.startCpuProfile([options])`
- `worker.startHeapProfile([options])`
- `worker.ref()`
- `worker.unref()`
- `worker.terminate()`
- `worker[Symbol.asyncDispose]()`
- `worker.performance`
- `worker.performance.eventLoopUtilization()`
- `worker.resourceLimits`
- `worker.threadId`
- `worker.threadName`
- `worker.stdin`
- `worker.stdout`
- `worker.stderr`
- `'online'`
- `'message'`
- `'messageerror'`
- `'error'`
- `'exit'`
- `port.close()`
- `port.start()`
- `port.ref()`
- `port.unref()`
- `port.hasRef()`
- `'message'`
- `'messageerror'`
- `'close'`
- `new MessageChannel()`
- `channel.port1`
- `channel.port2`
- … and 10 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `worker_threads.parentPort` | `ffi:js_worker_threads_parent_port` |
| `worker.postMessage(value[, transferList])` | `manifest:worker_threads.postMessage` |
| `port.postMessage(value[, transferList])` | `manifest:worker_threads.postMessage` |
| `bc.postMessage(message)` | `manifest:worker_threads.postMessage` |

### node:net

**Gap APIs: 56** · Already covered: 22

#### Missing from Perry

- `net.isIP(input)`
- `net.isIPv4(input)`
- `net.isIPv6(input)`
- `net.getDefaultAutoSelectFamily()`
- `net.setDefaultAutoSelectFamily(value)`
- `net.getDefaultAutoSelectFamilyAttemptTimeout()`
- `net.setDefaultAutoSelectFamilyAttemptTimeout(value)`
- `new net.BlockList()`
- `blockList.addAddress(address[, type])`
- `blockList.addRange(start, end[, type])`
- `blockList.addSubnet(net, prefix[, type])`
- `blockList.check(address[, type])`
- `blockList.fromJSON(value)`
- `blockList.toJSON()`
- `blockList.rules`
- `BlockList.isBlockList(value)`
- `new net.SocketAddress([options])`
- `socketAddress.address`
- `socketAddress.family`
- `socketAddress.flowlabel`
- `socketAddress.port`
- `SocketAddress.parse(input)`
- `new net.Server([options][, connectionListener])`
- `server.getConnections(callback)`
- `server.ref()`
- `server.unref()`
- `server[Symbol.asyncDispose]()`
- `server.listening`
- `server.maxConnections`
- `server.dropMaxConnection`
- `socket.address()`
- `socket.destroySoon()`
- `socket.pause()`
- `socket.ref()`
- `socket.resetAndDestroy()`
- `socket.resume()`
- `socket.setEncoding([encoding])`
- `socket.setKeepAlive([enable][, initialDelay])`
- `socket.setNoDelay([noDelay])`
- `socket.setTimeout(timeout[, callback])`
- `socket.getTypeOfService()`
- `socket.setTypeOfService(tos)`
- `socket.unref()`
- `socket.autoSelectFamilyAttemptedAddresses`
- `socket.bufferSize`
- `socket.bytesRead`
- `socket.bytesWritten`
- `socket.connecting`
- `socket.destroyed`
- `socket.localAddress`
- … and 6 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `net.connect(options[, connectListener])` | `manifest:net.connect` |
| `net.connect(path[, connectListener])` | `manifest:net.connect` |
| `net.connect(port[, host][, connectListener])` | `manifest:net.connect` |
| `net.createConnection(options[, connectListener])` | `expr:NetCreateConnection` |
| `net.createConnection(path[, connectListener])` | `expr:NetCreateConnection` |
| `net.createConnection(port[, host][, connectListener])` | `expr:NetCreateConnection` |
| `net.createServer([options][, connectionListener])` | `expr:NetCreateServer` |
| `server.address()` | `ffi:js_net_server_address` |
| `server.close([callback])` | `ffi:js_net_server_close` |
| `server.listen(handle[, backlog][, callback])` | `ffi:js_net_server_listen` |
| `server.listen(options[, callback])` | `ffi:js_net_server_listen` |
| `server.listen(path[, backlog][, callback])` | `ffi:js_net_server_listen` |
| `server.listen([port[, host[, backlog]]][, callback])` | `ffi:js_net_server_listen` |
| `new net.Socket([options])` | `manifest:net.Socket` |
| `socket.connect(options[, connectListener])` | `manifest:net.connect` |
| … | 7 more covered APIs |

### node:stream/web

**Gap APIs: 56** · Already covered: 12

#### Missing from Perry

- `new ReadableStream([underlyingSource[, strategy]])`
- `readableStream.locked`
- `readableStream.cancel([reason])`
- `readableStream.getReader([options])`
- `readableStream.pipeThrough(transform[, options])`
- `readableStream.pipeTo(destination[, options])`
- `readableStream.tee()`
- `readableStream.values([options])`
- `readableStream[Symbol.asyncIterator]()`
- `new ReadableStreamDefaultReader(stream)`
- `new ReadableStreamBYOBReader(stream)`
- `byobReader.closed`
- `byobReader.read(view[, options])`
- `byobReader.cancel([reason])`
- `byobReader.releaseLock()`
- `controller.desiredSize`
- `controller.close()`
- `controller.enqueue([chunk])`
- `controller.error([error])`
- `byteController.byobRequest`
- `byteController.desiredSize`
- `byteController.close()`
- `byteController.enqueue(chunk)`
- `byteController.error([error])`
- `byobRequest.view`
- `byobRequest.respond(bytesWritten)`
- `byobRequest.respondWithNewView(view)`
- `new WritableStream([underlyingSink[, strategy]])`
- `writableStream.locked`
- `writableStream.abort([reason])`
- `writableStream.close()`
- `writableStream.getWriter()`
- `new WritableStreamDefaultWriter(stream)`
- `writableController.signal`
- `writableController.error([error])`
- `new TransformStream([transformer[, writableStrategy[, readableStrategy]]])`
- `transformStream.readable`
- `transformStream.writable`
- `transformController.desiredSize`
- `transformController.enqueue([chunk])`
- `transformController.error([reason])`
- `transformController.terminate()`
- `new ByteLengthQueuingStrategy(init)`
- `strategy.highWaterMark`
- `strategy.size`
- `new CountQueuingStrategy(init)`
- `new TextEncoderStream()`
- `textEncoderStream.encoding`
- `textEncoderStream.readable`
- `textEncoderStream.writable`
- … and 6 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `ReadableStream.from(iterable)` | `manifest:stream.from` |
| `reader.closed` | `ffi:js_reader_closed` |
| `reader.read()` | `ffi:js_reader_read` |
| `reader.cancel([reason])` | `ffi:js_reader_cancel` |
| `reader.releaseLock()` | `ffi:js_reader_release_lock` |
| `writer.closed` | `ffi:js_writer_closed` |
| `writer.desiredSize` | `ffi:js_writer_desired_size` |
| `writer.ready` | `ffi:js_writer_ready` |
| `writer.write([chunk])` | `ffi:js_writer_write` |
| `writer.close()` | `ffi:js_writer_close` |
| `writer.abort([reason])` | `ffi:js_writer_abort` |
| `writer.releaseLock()` | `ffi:js_writer_release_lock` |

### node:perf_hooks

**Gap APIs: 55** · Already covered: 1

#### Missing from Perry

- `performance.mark(name[, options])`
- `performance.measure(name[, startMarkOrOptions[, endMark]])`
- `performance.clearMarks([name])`
- `performance.clearMeasures([name])`
- `performance.clearResourceTimings([name])`
- `performance.getEntries()`
- `performance.getEntriesByName(name[, type])`
- `performance.getEntriesByType(type)`
- `performance.eventLoopUtilization([util1[, util2]])`
- `performance.setResourceTimingBufferSize(maxSize)`
- `performance.timerify(fn[, options])`
- `performance.markResourceTiming(...)`
- `performance.toJSON()`
- `performance.nodeTiming`
- `performance.timeOrigin`
- `entry.name`
- `entry.entryType`
- `entry.startTime`
- `entry.duration`
- `entry.detail`
- `entry.detail`
- `entry.flags`
- `entry.kind`
- `nodeStart`
- `v8Start`
- `environment`
- `bootstrapComplete`
- `loopStart`
- `loopExit`
- `idleTime`
- `uvMetricsInfo`
- `toJSON()`
- `new PerformanceObserver(callback)`
- `PerformanceObserver.supportedEntryTypes`
- `observer.observe(options)`
- `observer.disconnect()`
- `observer.takeRecords()`
- `list.getEntries()`
- `list.getEntriesByName(name[, type])`
- `list.getEntriesByType(type)`
- `histogram.mean`
- `histogram.stddev`
- `histogram.percentile(percentile)`
- `histogram.percentileBigInt(percentile)`
- `histogram.reset()`
- `histogram.enable()`
- `histogram.disable()`
- `histogram[Symbol.dispose]()`
- `histogram.record(val)`
- `histogram.recordDelta()`
- … and 5 more (see `runtime-parity.md` for the full list)

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `performance.now()` | `expr:PerformanceNow` |

### node:tls

**Gap APIs: 38** · Already covered: 15

#### Missing from Perry

- `tls.createSecurePair([context][, isServer][, requestCert][, rejectUnauthorized][, options])`
- `tls.createServer([options][, secureConnectionListener])`
- `server.addContext(hostname, context)`
- `server.address()`
- `server.close([callback])`
- `server.getTicketKeys()`
- `server.listen()`
- `server.setSecureContext(options)`
- `server.setTicketKeys(keys)`
- `new tls.TLSSocket(socket[, options])`
- `tlsSocket.authorized`
- `tlsSocket.authorizationError`
- `tlsSocket.encrypted`
- `tlsSocket.localAddress`
- `tlsSocket.localPort`
- `tlsSocket.remoteAddress`
- `tlsSocket.remoteFamily`
- `tlsSocket.remotePort`
- `tlsSocket.address()`
- `tlsSocket.disableRenegotiation()`
- `tlsSocket.enableTrace()`
- `tlsSocket.exportKeyingMaterial(length, label[, context])`
- `tlsSocket.getCertificate()`
- `tlsSocket.getCipher()`
- `tlsSocket.getEphemeralKeyInfo()`
- `tlsSocket.getFinished()`
- `tlsSocket.getPeerCertificate([detailed])`
- `tlsSocket.getPeerFinished()`
- `tlsSocket.getPeerX509Certificate()`
- `tlsSocket.getProtocol()`
- `tlsSocket.getSession()`
- `tlsSocket.getSharedSigalgs()`
- `tlsSocket.getTLSTicket()`
- `tlsSocket.getX509Certificate()`
- `tlsSocket.isSessionReused()`
- `tlsSocket.renegotiate(options, callback)`
- `tlsSocket.setKeyCert(context)`
- `tlsSocket.setMaxSendFragment(size)`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `tls.connect(options[, callback])` | `ffi:js_tls_connect` |
| `tls.connect(port[, host][, options][, callback])` | `ffi:js_tls_connect` |
| `tls.connect(path[, options][, callback])` | `ffi:js_tls_connect` |
| `tls.checkServerIdentity(hostname, cert)` | `manifest:tls.checkServerIdentity`; `test-parity/node-suite/tls/identity/check-server-identity.ts` |
| `tls.createSecureContext([options])` | `manifest:tls.createSecureContext`; `test-parity/node-suite/tls/context/secure-context.ts` |
| `tls.setDefaultCACertificates(certs)` | `manifest:tls.setDefaultCACertificates`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.getCACertificates([type])` | `manifest:tls.getCACertificates`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.getCiphers()` | `manifest:tls.getCiphers`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.DEFAULT_ECDH_CURVE` | `manifest:tls.DEFAULT_ECDH_CURVE`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.DEFAULT_MAX_VERSION` | `manifest:tls.DEFAULT_MAX_VERSION`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.DEFAULT_MIN_VERSION` | `manifest:tls.DEFAULT_MIN_VERSION`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.DEFAULT_CIPHERS` | `manifest:tls.DEFAULT_CIPHERS`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.rootCertificates` | `manifest:tls.rootCertificates`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.CLIENT_RENEG_LIMIT` | `manifest:tls.CLIENT_RENEG_LIMIT`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |
| `tls.CLIENT_RENEG_WINDOW` | `manifest:tls.CLIENT_RENEG_WINDOW`; `test-parity/node-suite/tls/helpers/inventory-and-ca.ts` |

### node:fs/promises

**Gap APIs: 41** · Already covered: 20

#### Missing from Perry

- `fsPromises.access(path[, mode])`
- `fsPromises.chmod(path, mode)`
- `fsPromises.chown(path, uid, gid)`
- `fsPromises.cp(src, dest[, options])`
- `fsPromises.glob(pattern[, options])`
- `fsPromises.lchmod(path, mode)`
- `fsPromises.lchown(path, uid, gid)`
- `fsPromises.lstat(path[, options])`
- `fsPromises.lutimes(path, atime, mtime)`
- `fsPromises.mkdtemp(prefix[, options])`
- `fsPromises.mkdtempDisposable(prefix[, options])`
- `fsPromises.open(path, flags[, mode])`
- `fsPromises.opendir(path[, options])`
- `fsPromises.realpath(path[, options])`
- `fsPromises.rmdir(path[, options])`
- `fsPromises.statfs(path[, options])`
- `fsPromises.truncate(path[, len])`
- `fsPromises.utimes(path, atime, mtime)`
- `fsPromises.watch(filename[, options])`
- `fsPromises.constants`
- `filehandle.fd`
- `filehandle.chmod(mode)`
- `filehandle.chown(uid, gid)`
- `filehandle.close()`
- `filehandle.datasync()`
- `filehandle.pull([...transforms][, options])`
- `filehandle.pullSync([...transforms][, options])`
- `filehandle.read(buffer, offset, length, position)`
- `filehandle.read([options])`
- `filehandle.read(buffer[, options])`
- `filehandle.readableWebStream([options])`
- `filehandle.readv(buffers[, position])`
- `filehandle.sync()`
- `filehandle.truncate(len)`
- `filehandle.utimes(atime, mtime)`
- `filehandle.write(buffer, offset[, length[, position]])`
- `filehandle.write(buffer[, options])`
- `filehandle.write(string[, position[, encoding]])`
- `filehandle.writev(buffers[, position])`
- `filehandle.writer([options])`
- `filehandle[Symbol.asyncDispose]()`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `fsPromises.appendFile(path, data[, options])` | `manifest:fs.appendFile` |
| `fsPromises.copyFile(src, dest[, mode])` | `manifest:fs.copyFile` |
| `fsPromises.link(existingPath, newPath)` | `manifest:fs.link` |
| `fsPromises.mkdir(path[, options])` | `manifest:fs.mkdir` |
| `fsPromises.readdir(path[, options])` | `manifest:fs.readdir` |
| `fsPromises.readFile(path[, options])` | `manifest:fs.readFile` |
| `fsPromises.readlink(path[, options])` | `manifest:fs.readlink` |
| `fsPromises.rename(oldPath, newPath)` | `manifest:fs.rename` |
| `fsPromises.rm(path[, options])` | `manifest:fs.rm` |
| `fsPromises.stat(path[, options])` | `manifest:fs.stat` |
| `fsPromises.symlink(target, path[, type])` | `manifest:fs.symlink` |
| `fsPromises.unlink(path)` | `manifest:fs.unlink` |
| `fsPromises.writeFile(file, data[, options])` | `manifest:fs.writeFile` |
| `filehandle.appendFile(data[, options])` | `manifest:fs.appendFile` |
| `filehandle.createReadStream([options])` | `manifest:fs.createReadStream` |
| `filehandle.createWriteStream([options])` | `manifest:fs.createWriteStream` |
| `filehandle.readLines([options])` | `ffi:js_fs_filehandle_open` |
| `filehandle.readFile(options)` | `manifest:fs.readFile` |
| `filehandle.stat([options])` | `manifest:fs.stat` |
| `filehandle.writeFile(data, options)` | `manifest:fs.writeFile` |

### node:sqlite

**Gap APIs: 44** · Already covered: 8

#### Covered by Perry (#3183/#3184)

- `new DatabaseSync(path)` (incl. `:memory:`) → rusqlite connection
- `db.exec(sql)` / `db.prepare(sql)` → `StatementSync` / `db.close()`
- `stmt.run(...params)` → `{ changes, lastInsertRowid }`
- `stmt.get(...params)` / `stmt.all(...params)` → row object(s)
- `stmt.iterate(...params)` (array-backed) / `stmt.columns()` metadata

#### Missing from Perry

- `db.function(name[, options], fn)`
- `db.aggregate(name, options)`
- `db.applyChangeset(changeset[, options])`
- `db.createSession([options])`
- `db.createTagStore([maxSize])`
- `db.enableLoadExtension(allow)`
- `db.loadExtension(path)`
- `db.location([dbName])`
- `db.enableDefensive(active)`
- `db.serialize([dbName])`
- `db.deserialize(buffer[, options])`
- `db.setAuthorizer(callback)`
- `db.isOpen`
- `db.isTransaction`
- `db.limits`
- `db[Symbol.dispose]()`
- `stmt.setAllowBareNamedParameters(enabled)`
- `stmt.setAllowUnknownNamedParameters(enabled)`
- `stmt.setReadBigInts(enabled)`
- `stmt.setReturnArrays(enabled)`
- `stmt.sourceSQL`
- `stmt.expandedSQL`
- `session.changeset()`
- `session.patchset()`
- `session[Symbol.dispose]()`
- `tagStore.get`
- `tagStore.iterate`
- `tagStore.run`
- `tagStore.size`
- `tagStore.capacity`
- `tagStore.db`
- `tagStore.clear()`
- `sqlite.backup(sourceDb, path[, options])`
- `SQLITE_CHANGESET_DATA`
- `SQLITE_CHANGESET_NOTFOUND`
- `SQLITE_CHANGESET_CONFLICT`
- `SQLITE_CHANGESET_CONSTRAINT`
- `SQLITE_CHANGESET_FOREIGN_KEY`
- `SQLITE_CHANGESET_OMIT`
- `SQLITE_CHANGESET_REPLACE`
- `SQLITE_CHANGESET_ABORT`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `db.close()` | `ffi:js_sqlite_close` |
| `db.exec(sql)` | `ffi:js_sqlite_exec` |
| `db.prepare(sql[, options])` | `ffi:js_sqlite_prepare` |
| `db.open()` | `ffi:js_sqlite_open` |
| `stmt.all([namedParameters][, ...anonymousParameters])` | `ffi:js_sqlite_stmt_all` |
| `stmt.get([namedParameters][, ...anonymousParameters])` | `ffi:js_sqlite_stmt_get` |
| `stmt.run([namedParameters][, ...anonymousParameters])` | `ffi:js_sqlite_stmt_run` |
| `session.close()` | `ffi:js_sqlite_close` |

### node:url

**Gap APIs: 42** · Already covered: 7

#### Missing from Perry

- `new URL(input[, base])`
- `url.hash`
- `url.host`
- `url.hostname`
- `url.href`
- `url.origin`
- `url.password`
- `url.pathname`
- `url.port`
- `url.protocol`
- `url.search`
- `url.searchParams`
- `url.username`
- `url.toString()`
- `url.toJSON()`
- `URL.createObjectURL(blob)`
- `URL.revokeObjectURL(id)`
- `new URLSearchParams()`
- `new URLSearchParams(string)`
- `new URLSearchParams(obj)`
- `new URLSearchParams(iterable)`
- `params.append(name, value)`
- `params.delete(name[, value])`
- `params.entries()`
- `params.forEach(fn[, thisArg])`
- `params.get(name)`
- `params.getAll(name)`
- `params.has(name[, value])`
- `params.keys()`
- `params.set(name, value)`
- `params.size`
- `params.sort()`
- `params.toString()`
- `params.values()`
- `params[Symbol.iterator]()`
- `urlPattern.exec(input[, baseURL])`
- `urlPattern.test(input[, baseURL])`
- `url.domainToASCII(domain)`
- `url.domainToUnicode(domain)`
- `url.urlToHttpOptions(url)`
- `url.resolve(from, to)`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `URL.canParse(input[, base])` | `expr:UrlCanParse` |
| `URL.parse(input[, base])` | `manifest:url.parse` |
| `url.fileURLToPath(url[, options])` | `manifest:url.fileURLToPath` |
| `url.pathToFileURL(path[, options])` | `manifest:url.pathToFileURL` |
| `url.format(URL[, options])` | `manifest:url.format` |
| `url.parse(urlString[, parseQueryString[, slashesDenoteHost]])` | `manifest:url.parse` |
| `url.format(urlObject)` | `manifest:url.format` |

### node:buffer

**Gap APIs: 39** · Already covered: 69

#### Missing from Perry

- `Buffer.allocUnsafeSlow(size)`
- `Buffer.isEncoding(encoding)`
- `Buffer.poolSize`
- `buf.readBigInt64BE([offset])`
- `buf.readBigInt64LE([offset])`
- `buf.readBigUInt64BE([offset])`
- `buf.readBigUInt64LE([offset])`
- `buf.writeBigInt64BE(value[, offset])`
- `buf.writeBigInt64LE(value[, offset])`
- `buf.writeBigUInt64BE(value[, offset])`
- `buf.writeBigUInt64LE(value[, offset])`
- `buf.entries()`
- `buf.keys()`
- `buf.lastIndexOf(value[, start[, end]][, encoding])`
- `buf.subarray([start[, end]])`
- `buf.toJSON()`
- `buf.values()`
- `buf[index]`
- `buf.buffer`
- `buf.byteOffset`
- `buf.parent`
- `new buffer.Blob([sources[, options]])`
- `new buffer.File(sources, fileName[, options])`
- `file.name`
- `file.lastModified`
- `buffer.atob(data)`
- `buffer.btoa(data)`
- `buffer.isAscii(input)`
- `buffer.isUtf8(input)`
- `buffer.resolveObjectURL(id)`
- `buffer.transcode(source, fromEnc, toEnc)`
- `buffer.kMaxLength`
- `buffer.kStringMaxLength`
- `buffer.constants.MAX_LENGTH`
- `buffer.constants.MAX_STRING_LENGTH`
- `'base64'`
- `'base64url'`
- `'hex'`
- `'ascii'`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `Buffer.alloc(size[, fill[, encoding]])` | `manifest:buffer.alloc` |
| `Buffer.allocUnsafe(size)` | `manifest:buffer.allocUnsafe` |
| `Buffer.byteLength(string[, encoding])` | `manifest:buffer.byteLength` |
| `Buffer.compare(buf1, buf2)` | `ffi:js_buffer_compare` |
| `Buffer.concat(list[, totalLength])` | `manifest:buffer.concat` |
| `Buffer.copyBytesFrom(view[, offset[, length]])` | `ffi:js_buffer_copy_bytes_from` |
| `Buffer.from(array)` | `manifest:buffer.from` |
| `Buffer.from(arrayBuffer[, byteOffset[, length]])` | `manifest:buffer.from` |
| `Buffer.from(buffer)` | `manifest:buffer.from` |
| `Buffer.from(object[, offsetOrEncoding[, length]])` | `manifest:buffer.from` |
| `Buffer.from(string[, encoding])` | `manifest:buffer.from` |
| `Buffer.isBuffer(obj)` | `manifest:buffer.isBuffer` |
| `buffer.INSPECT_MAX_BYTES` | `manifest:buffer.INSPECT_MAX_BYTES` |
| `buf.readDoubleBE([offset])` | `ffi:js_buffer_read_double_be` |
| `buf.readDoubleLE([offset])` | `ffi:js_buffer_read_double_le` |
| `buf.readFloatBE([offset])` | `ffi:js_buffer_read_float_be` |
| `buf.readFloatLE([offset])` | `ffi:js_buffer_read_float_le` |
| … | 52 more covered APIs |

### node:events

**Gap APIs: 36** · Already covered: 5

#### Missing from Perry

- `EventEmitter.prototype.addListener(eventName, listener)`
- `EventEmitter.prototype.eventNames()`
- `EventEmitter.prototype.getMaxListeners()`
- `EventEmitter.prototype.listenerCount(eventName[, listener])`
- `EventEmitter.prototype.listeners(eventName)`
- `EventEmitter.prototype.off(eventName, listener)`
- `EventEmitter.prototype.once(eventName, listener)`
- `EventEmitter.prototype.prependListener(eventName, listener)`
- `EventEmitter.prototype.prependOnceListener(eventName, listener)`
- `EventEmitter.prototype.rawListeners(eventName)`
- `EventEmitter.prototype.setMaxListeners(n)`
- `EventEmitter.prototype[Symbol.for('nodejs.rejection')]()`
- `EventEmitterAsyncResource.prototype.asyncId`
- `EventEmitterAsyncResource.prototype.asyncResource`
- `EventEmitterAsyncResource.prototype.triggerAsyncId`
- `EventEmitterAsyncResource.prototype.emitDestroy()`
- `Event.prototype.composedPath()`
- `Event.prototype.initEvent(type, bubbles, cancelable)`
- `Event.prototype.preventDefault()`
- `Event.prototype.stopImmediatePropagation()`
- `Event.prototype.stopPropagation()`
- `EventTarget.prototype.addEventListener(type, listener[, options])`
- `EventTarget.prototype.dispatchEvent(event)`
- `EventTarget.prototype.removeEventListener(type, listener[, options])`
- `events.once(emitter, name[, options])`
- `events.getEventListeners(emitterOrTarget, eventName)`
- `events.getMaxListeners(emitterOrTarget)`
- `events.setMaxListeners(n[, ...eventTargets])`
- `events.listenerCount(emitterOrTarget, eventName)`
- `events.addAbortListener(signal, listener)`
- `events.defaultMaxListeners`
- `events.errorMonitor`
- `events.captureRejections`
- `events.captureRejectionSymbol`
- `'newListener'`
- `'removeListener'`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `EventEmitter.prototype.emit(eventName[, ...args])` | `manifest:events.emit` |
| `EventEmitter.prototype.on(eventName, listener)` | `manifest:events.on` |
| `EventEmitter.prototype.removeAllListeners([eventName])` | `manifest:events.removeAllListeners` |
| `EventEmitter.prototype.removeListener(eventName, listener)` | `manifest:events.removeListener` |
| `events.on(emitter, eventName[, options])` | `manifest:events.on` |

### node:child_process

**Gap APIs: 28** · Already covered: 9

#### Missing from Perry

- `subprocess.channel`
- `subprocess.connected`
- `subprocess.exitCode`
- `subprocess.killed`
- `subprocess.pid`
- `subprocess.signalCode`
- `subprocess.spawnargs`
- `subprocess.spawnfile`
- `subprocess.stdin`
- `subprocess.stdout`
- `subprocess.stderr`
- `subprocess.stdio`
- `subprocess.uid`
- `subprocess.gid`
- `subprocess.kill([signal])`
- `subprocess.send(message[, sendHandle[, options]][, callback])`
- `subprocess.disconnect()`
- `subprocess.ref()`
- `subprocess.unref()`
- `subprocess[Symbol.dispose]()`
- `'spawn'`
- `'error'`
- `'exit'`
- `'close'`
- `'disconnect'`
- `'message'`
- `child_process.ChildProcess`
- `child_process.Stream`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `child_process.spawn(command[, args][, options])` | `manifest:child_process.spawn` |
| `child_process.exec(command[, options][, callback])` | `manifest:child_process.exec` |
| `child_process.execFile(file[, args][, options][, callback])` | `manifest:child_process.execFile` |
| `child_process.fork(modulePath[, args][, options])` | `manifest:child_process.fork` |
| `child_process.spawnSync(command[, args][, options])` | `manifest:child_process.spawnSync` |
| `child_process.execSync(command[, options])` | `manifest:child_process.execSync` |
| `child_process.execFileSync(file[, args][, options])` | `manifest:child_process.execFileSync` |
| `util.promisify(exec)` | `manifest:util.promisify` |
| `util.promisify(execFile)` | `manifest:util.promisify` |

### node:assert (and node:assert/strict)

**Gap APIs: 26** · Already covered: 1

#### Missing from Perry

- `assert(value[, message])`
- `assert.fail([message])`
- `assert.equal(actual, expected[, message])`
- `assert.strictEqual(actual, expected[, message])`
- `assert.notEqual(actual, expected[, message])`
- `assert.notStrictEqual(actual, expected[, message])`
- `assert.deepEqual(actual, expected[, message])`
- `assert.deepStrictEqual(actual, expected[, message])`
- `assert.notDeepEqual(actual, expected[, message])`
- `assert.notDeepStrictEqual(actual, expected[, message])`
- `assert.partialDeepStrictEqual(actual, expected[, message])`
- `assert.match(string, regexp[, message])`
- `assert.doesNotMatch(string, regexp[, message])`
- `assert.throws(fn[, error][, message])`
- `assert.doesNotThrow(fn[, error][, message])`
- `assert.rejects(asyncFn[, error][, message])`
- `assert.doesNotReject(asyncFn[, error][, message])`
- `assert.ifError(value)`
- `assert.AssertionError`
- `assert.Assert`
- `assert.CallTracker`
- `tracker.calls(fn[, exact])`
- `tracker.getCalls(fn)`
- `tracker.report()`
- `tracker.reset([fn])`
- `tracker.verify()`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `assert.ok(value[, message])` | `rt:js_console_assert (partial)` |

### node:readline

**Gap APIs: 10** · Already covered: 19

#### Missing from Perry

- `rl[Symbol.dispose]()`
- `rl[Symbol.asyncIterator]()`
- `rl.cursor`
- `'pause'`
- `'resume'`
- `'history'`
- `'SIGINT'`
- `'SIGTSTP'`
- `'SIGCONT'`
- `'error'`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `readline.createInterface(options)` | `ffi:js_readline_create_interface` |
| `readline.clearLine(stream, dir[, callback])` | `rt:js_readline_clear_line_args`; `test-parity/node-suite/readline/helpers/terminal-helpers.ts` |
| `readline.clearScreenDown(stream[, callback])` | `rt:js_readline_clear_screen_down_args`; `test-parity/node-suite/readline/helpers/terminal-helpers.ts` |
| `readline.cursorTo(stream, x[, y][, callback])` | `rt:js_readline_cursor_to_args`; `test-parity/node-suite/readline/helpers/terminal-helpers.ts` |
| `readline.moveCursor(stream, dx, dy[, callback])` | `rt:js_readline_move_cursor_args`; `test-parity/node-suite/readline/helpers/terminal-helpers.ts` |
| `readline.emitKeypressEvents(stream[, interface])` | `rt:js_readline_emit_keypress_events_args`; `test-parity/node-suite/readline/helpers/emit-keypress-events.ts` |
| `rl.close()` | `manifest:readline.close` |
| `rl.pause()` | `ffi:js_readline_pause`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.resume()` | `ffi:js_readline_resume`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.prompt([preserveCursor])` | `ffi:js_readline_prompt`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.setPrompt(prompt)` | `ffi:js_readline_set_prompt`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.getPrompt()` | `ffi:js_readline_get_prompt`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.question(query[, options], callback)` | `manifest:readline.question` |
| `rl.write(data[, key])` | `ffi:js_readline_write`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.getCursorPos()` | `ffi:js_readline_get_cursor_pos`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.line` | `ffi:js_readline_line`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `rl.terminal` | `ffi:js_readline_terminal`; `test-parity/node-suite/readline/interface/control-methods.ts` |
| `'line'` | `ffi:js_readline_on`; `test-parity/node-suite/readline/interface/stream-line-close-events.ts` |
| `'close'` | `ffi:js_readline_on`; `test-parity/node-suite/readline/interface/stream-line-close-events.ts` |

### node:async_hooks

**Gap APIs: 23** · Already covered: 6

#### Missing from Perry

- `async_hooks.createHook(options)`
- `async_hooks.executionAsyncId()`
- `async_hooks.executionAsyncResource()`
- `async_hooks.triggerAsyncId()`
- `init(asyncId, type, triggerAsyncId, resource)`
- `before(asyncId)`
- `after(asyncId)`
- `destroy(asyncId)`
- `promiseResolve(asyncId)`
- `async_hooks.asyncWrapProviders`
- `asyncHook.enable()`
- `new AsyncLocalStorage()`
- `als.bind(fn)`
- `als.snapshot()`
- `AsyncLocalStorage.bind(fn)`
- `AsyncLocalStorage.snapshot()`
- `new AsyncResource(type[, options])`
- `ar.asyncId()`
- `ar.triggerAsyncId()`
- `ar.runInAsyncScope(fn, thisArg, ...args)`
- `ar.emitDestroy()`
- `ar.bind(fn)`
- `AsyncResource.bind(fn)`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `asyncHook.disable()` | `manifest:async_hooks.disable` |
| `als.getStore()` | `manifest:async_hooks.getStore` |
| `als.run(store, callback, ...args)` | `manifest:async_hooks.run` |
| `als.exit(callback, ...args)` | `manifest:async_hooks.exit` |
| `als.enterWith(store)` | `manifest:async_hooks.enterWith` |
| `als.disable()` | `manifest:async_hooks.disable` |

### node:wasi

**Gap APIs: 3** · Already covered: 3

#### Missing from Perry

- `wasi.start(instance)`
- `wasi.initialize(instance)`
- `wasi.finalizeBindings(instance[, options])`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `new WASI([options])` | `manifest:wasi.WASI`; `test-parity/node-suite/wasi/classes/constructor-validation.ts` |
| `wasi.getImportObject()` | `manifest:wasi.getImportObject`; `test-parity/node-suite/wasi/classes/import-object.ts` |
| `wasi.wasiImport` | `manifest:wasi.wasiImport`; `test-parity/node-suite/wasi/classes/import-object.ts` |

### node:module

**Gap APIs: 26** · Already covered: 13

#### Missing from Perry

- `Module.createRequire(filename)`
- `Module.getSourceMapsSupport()`
- `Module.register(specifier[, parentURL][, options])`
- `Module.registerHooks(options)`
- `Module.runMain()`
- `Module.setSourceMapsSupport(enabled[, options])`
- `Module.stripTypeScriptTypes(code[, options])`
- `Module.syncBuiltinESMExports()`
- `Module.wrap(code)`
- `Module.wrapper`
- `module.children`
- `module.exports`
- `module.filename`
- `module.id`
- `module.loaded`
- `module.parent`
- `module.path`
- `module.paths`
- `module.isPreloading`
- `module.require(id)`
- `module.load()`
- `require.cache` overrides
- `module._extensions`
- `module._cache`
- `module._pathCache`
- Customization hook callbacks

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `Module.builtinModules` | `manifest:module.builtinModules`; `runtime:js_module_builtin_modules` |
| `Module.findPackageJSON(specifier[, base])` | `manifest:module.findPackageJSON`; `runtime:js_module_find_package_json` |
| `Module.findSourceMap(path)` | `manifest:module.findSourceMap`; `test-parity/node-suite/module/source-map/basic.ts` |
| `Module.flushCompileCache()` | `manifest:module.flushCompileCache`; `test-parity/node-suite/module/compile-cache/controls.ts` |
| `Module.getCompileCacheDir()` | `manifest:module.getCompileCacheDir`; `test-parity/node-suite/module/compile-cache/controls.ts` |
| `Module.isBuiltin(moduleName)` | `manifest:module.isBuiltin`; `test-parity/node-suite/module/methods/is-builtin.ts` |
| `Module.constants.compileCacheStatus` | `manifest:module.constants`; `test-parity/node-suite/module/compile-cache/controls.ts` |
| `Module.enableCompileCache([cacheDir])` | `manifest:module.enableCompileCache`; `test-parity/node-suite/module/compile-cache/controls.ts` |
| `new SourceMap(payload[, options])` | `manifest:module.SourceMap`; `test-parity/node-suite/module/source-map/basic.ts` |
| `sourceMap.payload` | `test-parity/node-suite/module/source-map/basic.ts` |
| `sourceMap.findEntry(lineOffset, columnOffset)` | `test-parity/node-suite/module/source-map/basic.ts` |
| `sourceMap.findOrigin(lineNumber, columnNumber)` | `test-parity/node-suite/module/source-map/basic.ts` |
| Module namespace callable helper exports | `test-parity/node-suite/module/namespace/helper-shape.ts` |

### node:tty

**Gap APIs: 2** · Already covered: 17

#### Missing from Perry

- `readStream.fd`
- `writeStream.fd`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `tty.isatty(fd)` | `manifest:tty.isatty` |
| `new tty.ReadStream(fd[, options])` | `manifest:tty.ReadStream`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `readStream.isRaw` | `test-parity/fixtures/tty-pty-smoke.ts` |
| `readStream.isTTY` | `test-parity/fixtures/tty-pty-smoke.ts` |
| `readStream.setRawMode(mode)` | `manifest:tty.setRawMode`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `new tty.WriteStream(fd)` | `manifest:tty.WriteStream`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.isTTY` | `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.columns` | `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.rows` | `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.clearLine(dir[, callback])` | `manifest:tty.clearLine`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.clearScreenDown([callback])` | `manifest:tty.clearScreenDown`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.cursorTo(x[, y][, callback])` | `manifest:tty.cursorTo`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.moveCursor(dx, dy[, callback])` | `manifest:tty.moveCursor`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.getWindowSize()` | `manifest:tty.getWindowSize`; `test-parity/fixtures/tty-pty-smoke.ts` |
| `writeStream.getColorDepth([env])` | `manifest:tty.getColorDepth`; `test-parity/node-suite/tty/classes/color-depth-env.ts` |
| `writeStream.hasColors([count][, env])` | `manifest:tty.hasColors`; `test-parity/node-suite/tty/classes/has-colors-with-count.ts` |
| `'resize'` | `manifest:tty.on`; `test-parity/fixtures/tty-pty-smoke.ts` |

### node:https

**Gap APIs: 16** · Already covered: 7

#### Missing from Perry

- `https.globalAgent`
- `new https.Agent([options])`
- `agent.createConnection(options[, callback])`
- `agent.keepSocketAlive(socket)`
- `agent.reuseSocket(socket, request)`
- `agent.destroy()`
- `agent.getName([options])`
- `server.closeAllConnections()`
- `server.closeIdleConnections()`
- `server.setTimeout([msecs][, callback])`
- `server.headersTimeout`
- `server.maxHeadersCount`
- `server.requestTimeout`
- `server.timeout`
- `server.keepAliveTimeout`
- `server[Symbol.asyncDispose]()`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `https.createServer([options][, requestListener])` | `manifest:https.createServer` |
| `https.get(options[, callback])` | `manifest:https.get` |
| `https.get(url[, options][, callback])` | `manifest:https.get` |
| `https.request(options[, callback])` | `manifest:https.request` |
| `https.request(url[, options][, callback])` | `manifest:https.request` |
| `server.close([callback])` | `manifest:https.close` |
| `server.listen()` | `manifest:https.listen` |

### node:timers

**Gap APIs: 13** · Already covered: 4

#### Missing from Perry

- `setImmediate(callback[, ...args])`
- `clearImmediate(immediate)`
- `immediate.ref()`
- `immediate.unref()`
- `immediate.hasRef()`
- `immediate[Symbol.dispose]()`
- `timeout.ref()`
- `timeout.unref()`
- `timeout.hasRef()`
- `timeout.refresh()`
- `timeout.close()`
- `timeout[Symbol.toPrimitive]()`
- `timeout[Symbol.dispose]()`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `setInterval(callback[, delay[, ...args]])` | `ffi:js_interval_timer_*` |
| `setTimeout(callback[, delay[, ...args]])` | `ffi:js_set_timeout` |
| `clearInterval(timeout)` | `rt:js_interval_timer_*` |
| `clearTimeout(timeout)` | `rt:js_timer_*` |

### node:console

**Gap APIs: 6** · Already covered: 17

#### Missing from Perry

- `new Console(stdout[, stderr][, ignoreErrors])`
- `console.dirxml(...data)`
- `console.groupCollapsed()`
- `console.profile([label])`
- `console.profileEnd([label])`
- `console.timeStamp([label])`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `console.assert(value[, ...message])` | `ffi:js_console_assert` |
| `console.clear()` | `ffi:js_console_clear` |
| `console.count([label])` | `ffi:js_console_count` |
| `console.countReset([label])` | `ffi:js_console_count_reset` |
| `console.debug(data[, ...args])` | `rt:js_console_debug` |
| `console.dir(obj[, options])` | `rt:js_console_dir` |
| `console.error([data][, ...args])` | `rt:js_console_error` |
| `console.group([...label])` | `ffi:js_console_group` |
| `console.groupEnd()` | `ffi:js_console_group_end` |
| `console.info([data][, ...args])` | `rt:js_console_info` |
| `console.log([data][, ...args])` | `ffi:js_console_log` |
| `console.table(tabularData[, properties])` | `ffi:js_console_table` |
| `console.time([label])` | `ffi:js_console_time` |
| `console.timeEnd([label])` | `ffi:js_console_time_end` |
| `console.timeLog([label][, ...data])` | `ffi:js_console_time_log` |
| … | 2 more covered APIs |

### node:path

**Gap APIs: 3** · Already covered: 13

#### Missing from Perry

- `path.toNamespacedPath(path)`
- `path.posix`
- `path.win32`

#### Covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `path.basename(path[, suffix])` | `manifest:path.basename` |
| `path.dirname(path)` | `manifest:path.dirname` |
| `path.extname(path)` | `manifest:path.extname` |
| `path.format(pathObject)` | `manifest:path.format` |
| `path.isAbsolute(path)` | `manifest:path.isAbsolute` |
| `path.join([...paths])` | `manifest:path.join` |
| `path.matchesGlob(path, pattern)` | `manifest:path.matchesGlob`; `test-parity/node-suite/path/matchesGlob/extglob-globstar.ts`; `test-parity/node-suite/path/matchesGlob/win32-separators.ts` |
| `path.normalize(path)` | `manifest:path.normalize` |
| `path.parse(path)` | `manifest:path.parse` |
| `path.relative(from, to)` | `manifest:path.relative` |
| `path.resolve([...paths])` | `manifest:path.resolve` |
| `path.delimiter` | `expr:PathDelimiter` |
| `path.sep` | `expr:PathSep` |

## Web globals

**Total APIs: 389** · Perry covers: 107 · Gap: 282

Web-global coverage is determined heuristically — Perry implements many of these via dedicated `Expr::*` lowering (e.g. `Expr::FetchWithOptions`, `Expr::TextEncoderEncode`, `Expr::UrlNew`) and `js_*` FFI surfaces (Headers/Request/Response/Blob via perry-ext-fetch and perry-stdlib). The covered list below is curated; the gap list is everything else in the parity reference's Web / Global APIs section.

### Web globals — covered (sampled)

| API | Coverage source |
|-----|-----------------|
| `globalThis` | `builtin` |
| `console` | `builtin` |
| `performance` | `expr:PerformanceNow` |
| `queueMicrotask(cb)` | `rt:promise microtask queue` |
| `structuredClone(value, options?)` | `rt:js_structured_clone` |
| `atob(b64)` | `expr:Atob` |
| `btoa(str)` | `expr:Btoa` |
| `fetch(input, init?)` | `expr:FetchWithOptions` |
| `setTimeout(cb, ms, ...args)` | `ffi:js_set_timeout` |
| `setInterval(cb, ms, ...args)` | `ffi:js_interval_timer_*` |
| `clearTimeout(id)` | `rt:js_timer_*` |
| `clearInterval(id)` | `rt:js_interval_timer_*` |
| `__dirname` | `rt builtin` |
| `__filename` | `rt builtin` |
| `require(id)` | `compile-time resolution` |
| `process` | `expr:Process*` |
| `Buffer` | `expr:Buffer*` |
| `new URL(input, base?)` | `expr:UrlNew` |
| `URL.canParse(input, base?)` | `expr:UrlCanParse` |
| `URL.parse(input, base?)` | `expr:UrlParse` |
| `url.href` | `expr:UrlGetHref` |
| `url.origin` | `expr:UrlGetOrigin` |
| `url.protocol` | `expr:UrlGetProtocol` |
| `url.host` | `expr:UrlGetHost` |
| `url.hostname` | `expr:UrlGetHostname` |
| `url.port` | `expr:UrlGetPort` |
| `url.pathname` | `expr:UrlGetPathname` |
| `url.search` | `expr:UrlGetSearch` |
| `url.searchParams` | `expr:UrlGetSearchParams` |
| `url.hash` | `expr:UrlGetHash` |
| `url.toString()` | `expr:UrlInstanceToString` |
| `url.toJSON()` | `expr:UrlInstanceToJSON` |
| `new URLSearchParams(init?)` | `expr:UrlSearchParamsNew` |
| `usp.append(name, value)` | `expr:UrlSearchParamsAppend` |
| `usp.delete(name, value?)` | `expr:UrlSearchParamsDelete` |
| `usp.entries()` | `expr:UrlSearchParamsEntries` |
| `usp.get(name)` | `expr:UrlSearchParamsGet` |
| `usp.getAll(name)` | `expr:UrlSearchParamsGetAll` |
| `usp.has(name, value?)` | `expr:UrlSearchParamsHas` |
| `usp.set(name, value)` | `expr:UrlSearchParamsSet` |
| `usp.toString()` | `expr:UrlSearchParamsToString` |
| `new Headers(init?)` | `ffi:js_headers_new` |
| `headers.append(name, value)` | `ffi:js_headers_*` |
| `headers.delete(name)` | `ffi:js_headers_*` |
| `headers.get(name)` | `ffi:js_headers_*` |
| `headers.has(name)` | `ffi:js_headers_*` |
| `headers.set(name, value)` | `ffi:js_headers_*` |
| `headers.forEach(cb)` | `ffi:js_headers_for_each` |
| `headers.entries()` | `ffi:js_headers_entries` |
| `headers.keys()` | `ffi:js_headers_keys` |
| `headers.values()` | `ffi:js_headers_values` |
| `new Request(input, init?)` | `ffi:js_request_new` |
| `request.method` | `ffi:js_request_get_method` |
| `request.url` | `ffi:js_request_get_url` |
| `request.headers` | `ffi:js_request_get_headers` |
| `request.body` | `ffi:js_request_get_body` |
| `request.bodyUsed` | `ffi:js_request_body_used` |
| `request.arrayBuffer()` | `ffi:js_request_array_buffer` |
| `request.clone()` | `ffi:js_request_clone` |
| `request.json()` | `ffi:js_request_json` |
| `request.text()` | `ffi:js_request_text` |
| `new Response(body?, init?)` | `ffi:js_response_new` |
| `response.body` | `ffi:js_response_body` |
| `response.bodyUsed` | `ffi:js_response_body_used` |
| `response.headers` | `ffi:js_response_get_headers` |
| `response.ok` | `ffi:js_fetch_response_ok` |
| `response.status` | `ffi:js_fetch_response_status` |
| `response.statusText` | `ffi:js_fetch_response_status_text` |
| `response.arrayBuffer()` | `ffi:js_response_array_buffer` |
| `response.blob()` | `ffi:js_response_blob` |
| `response.clone()` | `ffi:js_response_clone` |
| `response.json()` | `ffi:js_fetch_response_json` |
| `response.text()` | `ffi:js_fetch_response_text` |
| `new Blob(parts?, options?)` | `ffi:js_blob_new` |
| `blob.size` | `ffi:js_blob_size` |
| `blob.type` | `ffi:js_blob_type` |
| `blob.arrayBuffer()` | `ffi:js_blob_array_buffer` |
| `blob.bytes()` | `ffi:js_blob_bytes` |
| `blob.slice(start?, end?, type?)` | `ffi:js_blob_slice` |
| `blob.stream()` | `ffi:js_blob_stream` |
| `blob.text()` | `ffi:js_blob_text` |
| `new ReadableStream(underlyingSource?, queuingStrategy?)` | `partial #562` |
| `new WritableStream(underlyingSink?, queuingStrategy?)` | `partial #562` |
| `controller.signal` | `ffi:js_abort_controller_signal` |
| `new TransformStream(transformer?, writableStrategy?, readableStrategy?)` | `partial #562` |
| `new AbortController()` | `ffi:js_abort_controller_new` |
| `AbortSignal.timeout(ms)` | `ffi:js_abort_signal_timeout` |
| `new TextEncoder()` | `expr:TextEncoderNew` |
| `new TextDecoder(label?, options?)` | `expr:TextDecoderNew` |
| `new MessageChannel()` | `partial via worker_threads` |
| `new BroadcastChannel(name)` | `partial` |
| `new WebSocket(url, protocols?)` | `ffi:js_ws_connect` |
| `crypto.getRandomValues(typedArray)` | `manifest:crypto.getRandomValues` |
| `crypto.randomUUID()` | `expr:CryptoRandomUUID` |
| `performance.now()` | `expr:PerformanceNow` |
| `console.log(...args)` | `rt:js_console_log` |
| `console.info(...args)` | `rt:js_console_info` |
| `console.warn(...args)` | `rt:js_console_warn` |
| `console.error(...args)` | `rt:js_console_error` |
| `console.debug(...args)` | `rt:js_console_debug` |
| `console.trace(...args)` | `rt:js_console_trace` |
| `console.assert(cond, ...args)` | `rt:js_console_assert` |
| `console.count(label?)` | `rt:js_console_count` |
| `console.dir(obj, options?)` | `rt:js_console_dir` |
| `console.time(label?)` | `rt:js_console_time` |
| `console.timeEnd(label?)` | `rt:js_console_time_end` |
| `console[Symbol.asyncIterator]()` | `builtin` |

### Web globals — gaps

Total gaps: 282. First 100 entries:

- `reportError(err)`
- `setImmediate(cb, ...args)`
- `clearImmediate(id)`
- `alert(msg)`
- `confirm(msg)`
- `prompt(msg, default?)`
- `ShadowRealm`
- `URL.createObjectURL(blob)`
- `URL.revokeObjectURL(url)`
- `url.username`
- `url.password`
- `usp.forEach(cb, thisArg?)`
- `usp.keys()`
- `usp.size`
- `usp.sort()`
- `usp.values()`
- `usp[Symbol.iterator]()`
- `headers.getSetCookie()`
- `headers[Symbol.iterator]()`
- `headers.toJSON()`
- `headers.count`
- `headers.getAll(name)`
- `request.destination`
- `request.referrer`
- `request.referrerPolicy`
- `request.mode`
- `request.credentials`
- `request.cache`
- `request.redirect`
- `request.integrity`
- `request.keepalive`
- `request.signal`
- `request.duplex`
- `request.blob()`
- `request.bytes()`
- `request.formData()`
- `Response.error()`
- `Response.json(data, init?)`
- `Response.redirect(url, status?)`
- `response.redirected`
- `response.type`
- `response.url`
- `response.bytes()`
- `response.formData()`
- `blob.json()`
- `blob.formData()`
- `blob.name`
- `blob.lastModified`
- `new File(parts, name, options?)`
- `file.name`
- `file.lastModified`
- `file.webkitRelativePath`
- `new FormData()`
- `fd.append(name, value, filename?)`
- `fd.delete(name)`
- `fd.get(name)`
- `fd.getAll(name)`
- `fd.has(name)`
- `fd.set(name, value, filename?)`
- `fd.entries()`
- `fd.keys()`
- `fd.values()`
- `fd.forEach(cb)`
- `fd[Symbol.iterator]()`
- `ReadableStream.from(iterable)`
- `rs.locked`
- `rs.cancel(reason?)`
- `rs.getReader(options?)`
- `rs.pipeThrough(transform, options?)`
- `rs.pipeTo(dest, options?)`
- `rs.tee()`
- `rs.values(options?)`
- `rs[Symbol.asyncIterator]()`
- `reader.read()`
- `reader.releaseLock()`
- `reader.cancel(reason?)`
- `reader.closed`
- `byob.read(view, options?)`
- `byob.releaseLock()`
- `byob.cancel(reason?)`
- `controller.desiredSize`
- `controller.close()`
- `controller.enqueue(chunk)`
- `controller.error(e?)`
- `controller.byobRequest`
- `req.view`
- `req.respond(bytesWritten)`
- `req.respondWithNewView(view)`
- `ws.locked`
- `ws.abort(reason?)`
- `ws.close()`
- `ws.getWriter()`
- `writer.desiredSize`
- `writer.ready`
- `writer.closed`
- `writer.abort(reason?)`
- `writer.close()`
- `writer.releaseLock()`
- … and 184 more (see `runtime-parity.md` for the full list)

## Bun-only APIs

Bun's runtime-specific surface (e.g. `Bun.serve`, `bun:sqlite`, `bun:ffi`, `HTMLRewriter`, `Bun.$`). Perry treats these as out-of-scope: porting Bun-specific code typically means writing the equivalent against Node-style APIs or the corresponding `perry-ext-*` crate.

**Bun-only API count: 394**

High-visibility entries (full list in `runtime-parity.md`):

- `Bun.version`
- `server.stop(closeActive?)`
- `ServerWebSocket.send(data, compress?)`
- `BunFile.size`
- `FileSink.write(chunk)`
- `Subprocess.pid`
- `class Bun.Terminal`
- `Terminal.write(data)`
- `Socket.timeout(seconds)`
- `UDPSocket.send(data, port, address)`
- `new Bun.CryptoHasher(algorithm, key?)`
- `hasher.update(data, encoding?)`
- `glob.scan(rootOrOptions)`
- `new S3Client({accessKeyId, secretAccessKey, region?, endpoint?, bucket?, sessionToken?, virtualHostedStyle?, acl?})`
- `client.file(path, options?)`
- `class S3File extends Blob`
- `s3file.text() / json() / bytes() / arrayBuffer() / stream() / slice(start, end?)`
- `sql.savepoint(name, cb)`
- `cookie.name / value / domain / path / expires / secure / sameSite / partitioned / maxAge / httpOnly`
- `map.toJSON()`
- `map[Symbol.iterator]()`
- `$.env(envObject\|undefined)`
- `ShellOutput.text(encoding?)`
- `wv.navigate(url) / .setHTML(html) / .eval(js) / .bind(name, fn) / .destroy() / .show()`
- `sink.start(options?)`
- `describe(name, fn)`
- `test(name, fn, timeoutOrOptions?)`
- `it(name, fn, ...)`
- `beforeAll(fn)`
- `beforeEach(fn)`
- `afterAll(fn)`
- `afterEach(fn)`
- `setDefaultTimeout(ms)`
- `mock(fn)`
- `spyOn(obj, method)`
- `jest.fn(fn?)`
- `expect(value)`
- `import { Database } from "bun:sqlite"`
- `new Database(filename?, options?)`
- `Database.deserialize(buf)`

## Methodology

**Sources consulted (in priority order):**

1. **Manifest entries** — `crates/perry-api-manifest/src/entries.rs` lists ~590 `(module, method)` rows. A match on `(section_module, method_name)` counts as covered. Module aliases handled: `node:fs` → `fs`, `node:fs/promises` → `fs/promises`.
2. **`Expr::*` HIR variants** — `crates/perry-hir/src/ir.rs` defines ~301 stdlib-shaped enum variants. APIs like `os.platform` map to `Expr::OsPlatform`; `path.join` to `Expr::PathJoin`; `crypto.randomUUID` to `Expr::CryptoRandomUUID`. The matcher Pascal-cases the section module and method name and checks set membership.
3. **`js_*` FFI exports** — extracted via grep (catching both `pub extern "C"` and `pub unsafe extern "C"` patterns) across `perry-runtime/src/*.rs` (~984 fns), `perry-stdlib/src/**/*.rs` (~633), and the 35 `perry-ext-*` crates (~553). Match heuristics: `js_<module>_<method_snake>`, `js_<module>_<method>_sync`, and a class-instance variant `js_<module>_<class>_<method>`.
4. **Web globals + builtin-globals overlay** — handled separately via a curated mapping from symbols like `fetch`, `new URL`, `crypto.subtle.digest`, `setTimeout`/`clearTimeout`, `console.error` to their backing implementation. Anything not in the overlay is reported as a gap.

**Caveats:**

- **Module-level matching is lenient.** The manifest uses bare method names within a module, so `on()` matches across `EventEmitter`/`Server`/`Socket` without class disambiguation. A name match indicates Perry can *dispatch* the call, not that the receiver class is implemented or that behavior matches Node byte-for-byte.
- **An FFI export's existence does not imply full semantic coverage.** `js_buffer_concat` being exported does not mean `Buffer.concat(list, totalLength)` with the optional totalLength arg matches Node. Several gaps in the partial-module lists are option/overload divergences.
- **`class X` declaration rows are excluded from gap counts.** The parity reference lists each class as its own row (`class fs.Dir`) followed by its methods; we only count callable methods/properties as gap candidates.
- **Loose name matching was dropped.** An earlier attempt let any module's `close()` count against `fs.close`; in practice it false-positived against `mongodb.close`/`http.close`/`readline.close`. The current matcher requires the module names to align.
- **Web global coverage is curated, not derived.** The whitelist has been hand-mapped against `Expr::*` and FFI surfaces; APIs Perry implements but that aren't in the whitelist will appear in the gap list. Treat the web-global gap count as an upper bound.
- **Bun-only APIs are reported as informational.** They're never targeted directly by Perry; users porting Bun code rewrite against Node-style equivalents or `perry-ext-*` crates.

**Methodology decisions:**

- Module aliasing: `node:fs` and `fs` treated as the same module. `node:fs/promises` is its own module key but falls back to `fs` for manifest lookup.
- Sync/async variants are checked separately: `fs.readFileSync` does not credit `fs.readFile`. The FFI heuristic does try the `_sync` suffix to handle the common pattern.
- For instance methods (`dir.read`, `socket.write`): the matcher first treats the leading segment as the class name and tries `js_<module>_<class>_<method>`; if that fails it falls through to the generic dispatch table.
- A small builtin overlay covers symbols like `setTimeout`, `clearTimeout`, `console.error` that ship as `js_set_timeout` / `js_console_*` rather than `js_timers_*` / `js_console_*` — without it the matcher would mis-report those as gaps.
- `class X` rows in the parity doc are explicitly excluded from the total/gap counts — they're documentation markers for the table that follows.
