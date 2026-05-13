# Perry FFI Test Coverage

Generated: 2026-05-14T05:30:25Z

## Summary

- **Total FFI functions:** 1791
- **Covered by TypeScript fixtures:** 1791 (100.0%)
- **Covered by Rust tests:** 1438 (80.3%)
- **Covered by either TS or Rust:** 1791 (100.0%)
- **Uncovered by either TS or Rust:** 0

## Coverage by File

| File | Total | TS Covered | Rust Covered | Combined | TS Coverage | Combined Coverage |
|------|-------|------------|--------------|----------|-------------|-------------------|
| `crates/perry-runtime/src/arena.rs` | 4 | 4 | 4 | 4 | 100% | 100% |
| `crates/perry-runtime/src/arkts_callbacks.rs` | 8 | 8 | 3 | 8 | 100% | 100% |
| `crates/perry-runtime/src/array.rs` | 77 | 77 | 77 | 77 | 100% | 100% |
| `crates/perry-runtime/src/bigint.rs` | 28 | 28 | 28 | 28 | 100% | 100% |
| `crates/perry-runtime/src/box.rs` | 3 | 3 | 0 | 3 | 100% | 100% |
| `crates/perry-runtime/src/buffer.rs` | 77 | 77 | 77 | 77 | 100% | 100% |
| `crates/perry-runtime/src/builtins.rs` | 62 | 62 | 62 | 62 | 100% | 100% |
| `crates/perry-runtime/src/child_process.rs` | 8 | 8 | 8 | 8 | 100% | 100% |
| `crates/perry-runtime/src/closure.rs` | 57 | 57 | 57 | 57 | 100% | 100% |
| `crates/perry-runtime/src/color_parse.rs` | 1 | 1 | 1 | 1 | 100% | 100% |
| `crates/perry-runtime/src/date.rs` | 41 | 41 | 41 | 41 | 100% | 100% |
| `crates/perry-runtime/src/error.rs` | 17 | 17 | 9 | 17 | 100% | 100% |
| `crates/perry-runtime/src/event_pump.rs` | 2 | 2 | 2 | 2 | 100% | 100% |
| `crates/perry-runtime/src/exception.rs` | 8 | 8 | 1 | 8 | 100% | 100% |
| `crates/perry-runtime/src/fs.rs` | 24 | 24 | 8 | 24 | 100% | 100% |
| `crates/perry-runtime/src/gc.rs` | 12 | 12 | 12 | 12 | 100% | 100% |
| `crates/perry-runtime/src/geisterhand_registry.rs` | 24 | 24 | 0 | 24 | 100% | 100% |
| `crates/perry-runtime/src/i18n.rs` | 18 | 18 | 0 | 18 | 100% | 100% |
| `crates/perry-runtime/src/ios_game_loop.rs` | 3 | 3 | 1 | 3 | 100% | 100% |
| `crates/perry-runtime/src/json.rs` | 14 | 14 | 2 | 14 | 100% | 100% |
| `crates/perry-runtime/src/jsx.rs` | 2 | 2 | 2 | 2 | 100% | 100% |
| `crates/perry-runtime/src/lib.rs` | 6 | 6 | 1 | 6 | 100% | 100% |
| `crates/perry-runtime/src/map.rs` | 14 | 14 | 12 | 14 | 100% | 100% |
| `crates/perry-runtime/src/math.rs` | 27 | 27 | 0 | 27 | 100% | 100% |
| `crates/perry-runtime/src/media_playback.rs` | 15 | 15 | 0 | 15 | 100% | 100% |
| `crates/perry-runtime/src/net.rs` | 11 | 11 | 11 | 11 | 100% | 100% |
| `crates/perry-runtime/src/node_stream.rs` | 6 | 6 | 1 | 6 | 100% | 100% |
| `crates/perry-runtime/src/object.rs` | 81 | 81 | 81 | 81 | 100% | 100% |
| `crates/perry-runtime/src/os.rs` | 29 | 29 | 29 | 29 | 100% | 100% |
| `crates/perry-runtime/src/path.rs` | 16 | 16 | 6 | 16 | 100% | 100% |
| `crates/perry-runtime/src/plugin.rs` | 22 | 22 | 22 | 22 | 100% | 100% |
| `crates/perry-runtime/src/process.rs` | 4 | 4 | 0 | 4 | 100% | 100% |
| `crates/perry-runtime/src/promise.rs` | 36 | 36 | 22 | 36 | 100% | 100% |
| `crates/perry-runtime/src/proxy.rs` | 19 | 19 | 1 | 19 | 100% | 100% |
| `crates/perry-runtime/src/regex.rs` | 19 | 19 | 19 | 19 | 100% | 100% |
| `crates/perry-runtime/src/set.rs` | 11 | 11 | 11 | 11 | 100% | 100% |
| `crates/perry-runtime/src/static_plugins.rs` | 2 | 2 | 0 | 2 | 100% | 100% |
| `crates/perry-runtime/src/stdlib_stubs.rs` | 16 | 16 | 15 | 16 | 100% | 100% |
| `crates/perry-runtime/src/string.rs` | 59 | 59 | 59 | 59 | 100% | 100% |
| `crates/perry-runtime/src/symbol.rs` | 16 | 16 | 8 | 16 | 100% | 100% |
| `crates/perry-runtime/src/text.rs` | 4 | 4 | 0 | 4 | 100% | 100% |
| `crates/perry-runtime/src/thread.rs` | 5 | 5 | 3 | 5 | 100% | 100% |
| `crates/perry-runtime/src/timer.rs` | 18 | 18 | 10 | 18 | 100% | 100% |
| `crates/perry-runtime/src/tty.rs` | 8 | 8 | 8 | 8 | 100% | 100% |
| `crates/perry-runtime/src/tui/ffi.rs` | 32 | 32 | 30 | 32 | 100% | 100% |
| `crates/perry-runtime/src/tui/hooks.rs` | 26 | 26 | 26 | 26 | 100% | 100% |
| `crates/perry-runtime/src/tui/input.rs` | 2 | 2 | 2 | 2 | 100% | 100% |
| `crates/perry-runtime/src/tui/run.rs` | 1 | 1 | 1 | 1 | 100% | 100% |
| `crates/perry-runtime/src/tui/state.rs` | 3 | 3 | 3 | 3 | 100% | 100% |
| `crates/perry-runtime/src/typedarray.rs` | 15 | 15 | 12 | 15 | 100% | 100% |
| `crates/perry-runtime/src/ui_text_registry.rs` | 18 | 18 | 1 | 18 | 100% | 100% |
| `crates/perry-runtime/src/url.rs` | 36 | 36 | 36 | 36 | 100% | 100% |
| `crates/perry-runtime/src/value.rs` | 54 | 54 | 54 | 54 | 100% | 100% |
| `crates/perry-runtime/src/watchos_game_loop.rs` | 2 | 2 | 1 | 2 | 100% | 100% |
| `crates/perry-runtime/src/weakref.rs` | 15 | 15 | 1 | 15 | 100% | 100% |
| `crates/perry-runtime/src/webassembly.rs` | 7 | 7 | 0 | 7 | 100% | 100% |
| `crates/perry-stdlib/src/argon2.rs` | 5 | 5 | 2 | 5 | 100% | 100% |
| `crates/perry-stdlib/src/async_local_storage.rs` | 6 | 6 | 5 | 6 | 100% | 100% |
| `crates/perry-stdlib/src/axios.rs` | 8 | 8 | 8 | 8 | 100% | 100% |
| `crates/perry-stdlib/src/bcrypt.rs` | 5 | 5 | 2 | 5 | 100% | 100% |
| `crates/perry-stdlib/src/cheerio.rs` | 18 | 18 | 18 | 18 | 100% | 100% |
| `crates/perry-stdlib/src/commander.rs` | 15 | 15 | 15 | 15 | 100% | 100% |
| `crates/perry-stdlib/src/common/async_bridge.rs` | 2 | 2 | 1 | 2 | 100% | 100% |
| `crates/perry-stdlib/src/common/dispatch.rs` | 4 | 4 | 0 | 4 | 100% | 100% |
| `crates/perry-stdlib/src/common/handle.rs` | 1 | 1 | 1 | 1 | 100% | 100% |
| `crates/perry-stdlib/src/cron.rs` | 14 | 14 | 14 | 14 | 100% | 100% |
| `crates/perry-stdlib/src/crypto.rs` | 16 | 16 | 1 | 16 | 100% | 100% |
| `crates/perry-stdlib/src/crypto_e2e.rs` | 6 | 6 | 0 | 6 | 100% | 100% |
| `crates/perry-stdlib/src/dayjs.rs` | 36 | 36 | 36 | 36 | 100% | 100% |
| `crates/perry-stdlib/src/decimal.rs` | 42 | 42 | 42 | 42 | 100% | 100% |
| `crates/perry-stdlib/src/dotenv.rs` | 3 | 3 | 3 | 3 | 100% | 100% |
| `crates/perry-stdlib/src/ethers.rs` | 8 | 8 | 8 | 8 | 100% | 100% |
| `crates/perry-stdlib/src/events.rs` | 7 | 7 | 7 | 7 | 100% | 100% |
| `crates/perry-stdlib/src/exponential_backoff.rs` | 2 | 2 | 2 | 2 | 100% | 100% |
| `crates/perry-stdlib/src/fastify/app.rs` | 14 | 14 | 14 | 14 | 100% | 100% |
| `crates/perry-stdlib/src/fastify/context.rs` | 20 | 20 | 19 | 20 | 100% | 100% |
| `crates/perry-stdlib/src/fastify/server.rs` | 2 | 2 | 2 | 2 | 100% | 100% |
| `crates/perry-stdlib/src/fetch.rs` | 44 | 44 | 44 | 44 | 100% | 100% |
| `crates/perry-stdlib/src/framework/multipart.rs` | 2 | 2 | 2 | 2 | 100% | 100% |
| `crates/perry-stdlib/src/framework/request.rs` | 8 | 8 | 0 | 8 | 100% | 100% |
| `crates/perry-stdlib/src/framework/response.rs` | 8 | 8 | 0 | 8 | 100% | 100% |
| `crates/perry-stdlib/src/framework/server.rs` | 10 | 10 | 0 | 10 | 100% | 100% |
| `crates/perry-stdlib/src/http.rs` | 13 | 13 | 13 | 13 | 100% | 100% |
| `crates/perry-stdlib/src/ioredis.rs` | 18 | 18 | 18 | 18 | 100% | 100% |
| `crates/perry-stdlib/src/jsonwebtoken.rs` | 5 | 5 | 5 | 5 | 100% | 100% |
| `crates/perry-stdlib/src/lib.rs` | 2 | 2 | 2 | 2 | 100% | 100% |
| `crates/perry-stdlib/src/lodash.rs` | 38 | 38 | 16 | 38 | 100% | 100% |
| `crates/perry-stdlib/src/lru_cache.rs` | 8 | 8 | 8 | 8 | 100% | 100% |
| `crates/perry-stdlib/src/moment.rs` | 28 | 28 | 28 | 28 | 100% | 100% |
| `crates/perry-stdlib/src/mongodb.rs` | 26 | 26 | 26 | 26 | 100% | 100% |
| `crates/perry-stdlib/src/mysql2/connection.rs` | 7 | 7 | 7 | 7 | 100% | 100% |
| `crates/perry-stdlib/src/mysql2/pool.rs` | 8 | 8 | 8 | 8 | 100% | 100% |
| `crates/perry-stdlib/src/nanoid.rs` | 3 | 3 | 3 | 3 | 100% | 100% |
| `crates/perry-stdlib/src/net/mod.rs` | 10 | 10 | 10 | 10 | 100% | 100% |
| `crates/perry-stdlib/src/nodemailer.rs` | 3 | 3 | 3 | 3 | 100% | 100% |
| `crates/perry-stdlib/src/perry_ffi_async.rs` | 6 | 6 | 5 | 6 | 100% | 100% |
| `crates/perry-stdlib/src/pg/connection.rs` | 6 | 6 | 6 | 6 | 100% | 100% |
| `crates/perry-stdlib/src/pg/pool.rs` | 4 | 4 | 4 | 4 | 100% | 100% |
| `crates/perry-stdlib/src/ratelimit.rs` | 11 | 11 | 11 | 11 | 100% | 100% |
| `crates/perry-stdlib/src/readline.rs` | 8 | 8 | 8 | 8 | 100% | 100% |
| `crates/perry-stdlib/src/sharp.rs` | 18 | 18 | 18 | 18 | 100% | 100% |
| `crates/perry-stdlib/src/slugify.rs` | 3 | 3 | 3 | 3 | 100% | 100% |
| `crates/perry-stdlib/src/sqlite.rs` | 14 | 14 | 14 | 14 | 100% | 100% |
| `crates/perry-stdlib/src/streams.rs` | 39 | 39 | 36 | 39 | 100% | 100% |
| `crates/perry-stdlib/src/uuid.rs` | 6 | 6 | 6 | 6 | 100% | 100% |
| `crates/perry-stdlib/src/validator.rs` | 17 | 17 | 17 | 17 | 100% | 100% |
| `crates/perry-stdlib/src/webcrypto.rs` | 4 | 4 | 4 | 4 | 100% | 100% |
| `crates/perry-stdlib/src/worker_threads.rs` | 6 | 6 | 3 | 6 | 100% | 100% |
| `crates/perry-stdlib/src/ws.rs` | 23 | 23 | 23 | 23 | 100% | 100% |
| `crates/perry-stdlib/src/zlib.rs` | 6 | 6 | 6 | 6 | 100% | 100% |
