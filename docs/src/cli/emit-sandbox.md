# `--emit-sandbox` — Kernel-Enforced Sandbox Profile

When a Perry binary is built with `--emit-sandbox`, the compiler writes
a sandbox profile alongside the executable that the host can apply at
runtime. The profile is derived from the build's *reachable stdlib
surface* — a program that never imports `child_process` gets a profile
denying `fork`/`execve`; one that never imports `http`/`fetch`/`net`
gets a profile denying outbound network; etc.

**Zero per-call overhead in Perry's emitted code.** The kernel does
the syscall-entry check, which it already does for every syscall
regardless of sandbox state.

## Today: macOS only (MVP)

`perry compile --emit-sandbox main.ts -o myapp` writes:

- `myapp` — the executable.
- `myapp.sandbox` — a sandbox-exec profile derived from the build.

Apply at run time:

```bash
sandbox-exec -f myapp.sandbox myapp
```

Linux `seccomp` BPF + landlock, Windows AppContainer, and per-API
HIR-driven refinement are tracked as
[#506](https://github.com/PerryTS/perry/issues/506) follow-ups.

## Enabling

Priority order, last wins (mirrors `--fast-math` / `--lockdown`):

1. **CLI flag**: `perry compile --emit-sandbox ...`
2. **Env var**: `PERRY_EMIT_SANDBOX=1` (and `=0` explicitly disables).
3. **`package.json`**: `{ "perry": { "emitSandbox": true } }`.

## What's derived from the build

| Build signal                          | Effect on profile                           |
|---------------------------------------|---------------------------------------------|
| `import "child_process"`              | Allow `process-fork` + `process-exec`       |
| Anything in `http` / `https` / `net` / `tls` / `dns` / `ws` / `axios` / `node-fetch` / `redis` / `ioredis` | Allow `network*` |
| `fetch(...)` reachable                | Same as above                               |
| `import "fs"`                         | Allow `file-write*` under `/tmp`, `/private/tmp`, `/private/var/folders` |
| `perry-jsruntime` linked              | Allow `dynamic-code-generation` (QuickJS JIT) |
| Always                                | Deny default. Allow `file-read*` on system locations + `/dev/null` + `/dev/urandom` so the dynamic linker reaches `main()`. |

The generated profile is a *starting point* — review and tighten
manually for production builds. Per-API HIR-driven refinement (which
would distinguish `fs.readFileSync`-only deps from `fs.writeFileSync`
deps, or `fetch("https://api.example.com/...")` from `fetch(url)`)
lands as a follow-up under the same flag.

## Header documents itself

The emitted profile starts with a documentation header that shows the
`sandbox-exec -f ... ...` invocation and cites #506 for context — so
downstream operators can see immediately how to apply it without
hunting through Perry docs.

## Composition with `--lockdown`

When the `--lockdown` mode (#496) lands, it will default
`--emit-sandbox` on. For now they're orthogonal.

## What's NOT covered (MVP)

- Linux `seccomp` BPF filter + landlock FS scoping — follow-up.
- Windows AppContainer manifest — follow-up.
- Per-API HIR-driven refinement (`fs.readFileSync` ≠ `fs.writeFileSync`,
  literal-host extraction for `fetch`).
- Auto-loading the profile at process start via `sandbox_init` instead
  of the `sandbox-exec` wrapper.
- iOS / Android — already sandboxed by the platform at process launch;
  out of scope for this flag.

## See also

- [`#506`](https://github.com/PerryTS/perry/issues/506) — design discussion + tracker.
- The wider supply-chain hardening series
  ([`#495`–`#506`](https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
