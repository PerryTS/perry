# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.10](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.9...turnloop-v0.1.0-alpha.10) - 2026-10-24

### Other

- Connect owned bound and unbound TCP sockets through the existing driver ([#125](https://github.com/PerryTS/turnloop/pull/125))
- Expose independent TCP socket controls for native hosts ([#127](https://github.com/PerryTS/turnloop/pull/127))

## [0.1.0-alpha.9](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.8...turnloop-v0.1.0-alpha.9) - 2026-10-16

### Fixed

- build on Android (Bionic errno accessor, inotify_rm_watch descriptor type) ([#114](https://github.com/PerryTS/turnloop/pull/114))

## [0.1.0-alpha.8](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.7...turnloop-v0.1.0-alpha.8) - 2026-09-23

### Added

- adopt a host-created listening socket on every Unix backend ([#108](https://github.com/PerryTS/turnloop/pull/108))

## [0.1.0-alpha.7](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.6...turnloop-v0.1.0-alpha.7) - 2026-09-22

### Other

- pool-result ceiling ([#88](https://github.com/PerryTS/turnloop/pull/88)), multishot accept budget ([#77](https://github.com/PerryTS/turnloop/pull/77)), safe loop rebuild ([#43](https://github.com/PerryTS/turnloop/pull/43)) ([#106](https://github.com/PerryTS/turnloop/pull/106))
- Core host API: hand back host completions, TCP connect timeout, single-connection presets ([#105](https://github.com/PerryTS/turnloop/pull/105))

## [0.1.0-alpha.6](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.5...turnloop-v0.1.0-alpha.6) - 2026-09-17

### Other

- allocate a ring's slots zeroed instead of writing every one ([#89](https://github.com/PerryTS/turnloop/pull/89))
- Multi-threaded accept: a reuse-port request that names its promise, and the handoff proven ([#83](https://github.com/PerryTS/turnloop/pull/83))

## [0.1.0-alpha.5](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.4...turnloop-v0.1.0-alpha.5) - 2026-09-16

### Other

- Page the slot tables so a capacity ceiling costs nothing idle ([#76](https://github.com/PerryTS/turnloop/pull/76))
- A long-occupancy job class for the blocking pool ([#74](https://github.com/PerryTS/turnloop/pull/74))
- Reserve extra descriptor numbers across the fork
- Keep the child fixture buildable on targets without processes
- Pass extra child descriptors and control the child session at spawn

## [0.1.0-alpha.4](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.3...turnloop-v0.1.0-alpha.4) - 2026-09-15

### Other

- A Windows socket duplicate inherits its IOCP association
- Hand a quiescent transport's descriptor back to the host
- Assert a floor, not an exact size, for kernel-chosen socket buffers
- Refuse an unappliable accept default at the accept, not only at listen
- Document the keep-alive partial-failure contract
- Point the socket-option docs at DESIGN 7.7 and simplify a test helper
- Report a creation-time nodelay WASI cannot apply, and cover the option surface
- Add a typed socket-option API on live handles
- Normalize WASI deadline-event accounting (I02) ([#28](https://github.com/PerryTS/turnloop/pull/28))
- Normalize WASI deadline-event accounting (I02)

## [0.1.0-alpha.3](https://github.com/PerryTS/turnloop/compare/turnloop-v0.1.0-alpha.2...turnloop-v0.1.0-alpha.3) - 2026-09-15

### Other
- Queued turns never block; zero-timeout discovery polls are counted separately from blocking waits (DESIGN §10 rule 3)
- Typed filesystem requests and filesystem watches on every backend (epoll/inotify, kqueue/FSEvents, IOCP ReadDirectoryChangesW, WASI 0.2/0.3)
- Windows: Node/libuv process, console and synchronous-handle semantics (`windows_hide`, `detached`, parent-lifetime job, permanent console handler, duplex read preemption)
- `AsyncIo::poll_shutdown` half-closes the write side without closing the handle
- Published dependencies use caret requirements instead of exact pins

- named-pipe backlog, same-port pipe routing, Windows contract equivalents
