**The four database wrappers are group J: delete, do not convert.**

`perry-ext-pg`, `perry-ext-mysql2`, `perry-ext-ioredis` and `perry-ext-mongodb`
carry 8 of the 21 remaining tokio edges. None of them will be migrated to
turnloop, because none of them needs to exist: `pg`, `mysql2`, `ioredis` and
`mongodb` are pure-JS npm packages, and the native wrappers exist only because
Perry was TypeScript-only at one time. Perry compiles JS from source now, so the
wrappers get deleted and their edges leave with them.

That is recorded in `scripts/tokio_inventory.json` — the gated record every lane
reads — rather than in prose, because the failure mode here is a lane spending
days migrating a crate that is about to be removed. Each of the eight entries now
opens with the scope decision and keeps its old blocker underneath, marked no
longer actionable, so the history is not lost.

It also changes what "remove tokio" means. The residue after group J is not
wrappers at all — it is Perry's own runtime surface:

- `perry-ext-net` (2) — `node:net`
- `perry-ext-http` (5) — `node:http` / `https` / `http2`
- `perry-stdlib` (3) — `node:tls`, `fetch`, and `async_bridge::RUNTIME`
- `perry-container-compose` (2), a CLI tool, and `perry-ui-android` (1), which is
  sync tungstenite and not a tokio edge at all

There is no JS source to compile for a Node builtin, so those 11 are the real
work. `async_bridge::RUNTIME` still goes last by construction: it is tokio
because its clients hand it tokio futures, and it goes when the last of them
does.

One thing already built survives the change and matters more for it, not less:
the `agent_post` C ABI. The ioredis transport on top of it becomes moot, but the
ABI — letting a thread hand work to the thread that owns an agent's loop — is
what closes the decline path for `node:net` and `node:http`, which are staying.
