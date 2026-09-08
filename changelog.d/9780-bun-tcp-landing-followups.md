**`Bun.listen` / `Bun.connect` are documented, and `perry-ext-http`'s unit
tests link again.** #9514's TCP socket facades added both methods to the
compile-time API manifest without regenerating the derived docs, so
`docs/src/api/reference.md` and `docs/api/perry.d.ts` had been stale since
2026-09-03 and the API-docs drift check failed on every run. They now list
both entries (3046 → 3048).

Separately, `js_bun_tcp_listen` drives the shared async runtime from its
bind-poll loop via `perry_ffi::run_pending`. `perry-ext-net` stubs that
symbol only under `#[cfg(test)]`, which does not apply when it is linked as
an ordinary dependency into `perry-ext-http`'s test binary, so release-linking
that crate failed with `undefined symbol: perry_ffi_run_pending`.
`perry-ext-http`'s test shim now provides it, alongside the
`perry_ffi_spawn_async` stub that exists for the same transitive reason.
`perry-ext-ws` was checked and does not need it.
