# turnloop

An embeddable event-loop driver for Rust. Your host owns the thread and calls
`turn()` to advance I/O, timers and cross-thread posts. Each turn returns
completions, makes at most one OS wait, and runs no user callbacks. There is no
runtime thread and no tokio dependency.

**Pre-alpha.** This workspace contains the Unix driver and runtime-independent
PostgreSQL, MySQL, Redis, MongoDB and SMTP protocol engines. APIs can change.
Protocol engines consume bytes and produce actions; host transport adapters are still being integrated. The optional local executor
drives futures-io streams, sleep and timeout from host turns. Perry is the first intended consumer.

| Platform | Current implementation | Validation |
|---|---|---|
| macOS arm64 | kqueue, TCP/UDP/local IPC, stdio, processes, signals, TTY, timers and shared services | Native contracts, six allocation gates and executor tests |
| Linux x86_64 / arm64 | epoll, pidfd/SIGCHLD, native services, nanosecond waits and timerfd fallback | Cross-checked locally; native CI required |
| FreeBSD / Apple mobile / Android | Unix backend paths | Best effort; runtime validation pending |
| Windows x86_64 | IOCP, TCP/UDP/named pipes, stdio, processes, console signals, timers and shared services | Native Windows 11 contracts and allocation/executor tests; see `spikes/iocp/WINDOWS_RESULTS.md` |
| WASI 0.2 / 0.3 | Standalone polling / component async spikes | Production adapters and shared contracts pending |
| Web | Standalone host-callback spike | Production adapter and browser contracts pending |

Windows and WASM are required for the first release; standalone spikes are
excluded from the publishable workspace. The platform table describes current
code, not a completed support promise. HTTP, TLS and WebSocket crates are being
worked on separately.

```rust,no_run
use std::time::Duration;
use turnloop::{Completions, Config, Loop, Timeout, Token};

let mut driver = Loop::new(Config::default())?;
let deadline = driver.now() + Duration::from_millis(2);
driver.timer(deadline, None, Token(42))?;
let mut completions = Completions::default();
driver.turn(Timeout::Until(deadline), &mut completions)?;
// Dispatch completions in the host's own event-loop phase.
# Ok::<(), turnloop::Error>(())
```

Enable `executor` for `LocalExecutor<B>` and its futures-io adapters. Read/write
staging and task tables are reserved at construction. Writes are buffered; flush
or close before dropping an adapter to confirm underlying completion.
`AsyncIo::poll_shutdown` half-closes a stream without releasing its handle. A host
that also submits its own operations on the executor's loop turns it with
`LocalExecutor::turn_into`, which hands back every completion the executor did not
issue. The crate's
rustdoc includes runnable loop and executor examples. See the
[revision 2 handoff](https://github.com/PerryTS/turnloop/blob/main/docs/BACKEND_REVISION_2.md)
for native ownership, process teardown, platform integration and contract details.

Use the pinned nightly for dependency resolution under the seven-day publication
soak. The workspace also builds with stable Rust 1.97.1.

- [Design and host boundary](https://github.com/PerryTS/turnloop/blob/main/DESIGN.md)
- [Contributing, checks and private test servers](https://github.com/PerryTS/turnloop/blob/main/CONTRIBUTING.md)
- [Release and first-publication procedure](https://github.com/PerryTS/turnloop/blob/main/RELEASING.md)
- [Integration status and verification](https://github.com/PerryTS/turnloop/blob/main/docs/INTEGRATION_REPORT.md)
- [MIT license](https://github.com/PerryTS/turnloop/blob/main/LICENSE)
