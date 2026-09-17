### ioredis: a thread with no loop of its own posts to the thread that has one

`perry-ext-ioredis` had two transports — one turnloop socket plus a
`turnloop-redis` core, and a `redis`-crate fallback for clients that decline.
It now has three, and the middle one is the interesting one.

| transport | when | what carries a command |
|---|---|---|
| `Direct` | this thread owns the agent's loop | today's path, unchanged |
| `Posted` | **another thread of this same agent owns it** | `perry_ffi::agent_post::post_job` hands the command to that owner |
| `Legacy` | no loop exists for this agent at all | `spawn_blocking` + `Handle::current().block_on` |

`Posted` is the case the whole tokio dependency was being kept for. Since
turnloop P9 every JS agent has a loop, so "a thread that could not get a loop of
its own" stopped meaning "a worker" and started meaning "a **second thread
acting for an agent another thread already owns**" — the Android shape, where
`perry-native` runs the compiled TypeScript on the primary heap while the UI
thread pumps for the same heap, and whichever claims the route first leaves the
other unable to submit. Every `js_ioredis_*` entry point — the sixteen commands
through `tl_command`, plus `connect`, `quit` and `disconnect` — now routes
through the owner on that path instead of spinning up a blocking-pool thread.

Two things come free with it. The reply is built on the **owner**, which is
where that agent's JS values live — the #1824 rule the `spawn_blocking` path had
to obey by hand. And a posted command holds *owned* bytes: the borrowed
`&[u8]` slices a direct submission passes cannot cross a thread, so they are
copied once, at the post, and nowhere else.

**Settlement is preserved on every refusal.** A dropped `JsPromise` is a promise
that never settles, which is the one outcome a caller cannot recover from, so a
post that does not land hands the job back and the promise is rejected here:
`NoRoute` (the owner's loop went away) reads as a closed connection, `Again`
(its postbox is momentarily full) as back-pressure. There is deliberately no
fall back to the `redis` crate at command time — a client created on turnloop
may already have commands queued, and reordering them would break a MULTI block.

**Found while testing this, and worth knowing:** `net_available()` *claims* an
agent's route without building a loop — that is deliberate, so a thread can ask
"may I use turnloop?" without paying for a loop it may not use — but the
`Poster` is only published when the loop is actually built. A claim is nothing
to post to. So `agent_post::available()` answers the *published* question, not
the claimed one, and a client created in the window between a thread claiming
its slot and publishing its loop is created on the legacy transport for its
life. Narrow (the owner's own event loop publishes at startup, long before a
second thread creates a client) and documented rather than papered over.

**The test asserts the work crossed**, which is the only thing that makes the
claim non-vacuous: the runtime's dispatch counter is per-thread, so the
discriminating fact is that it moves on the owner while staying at **zero** on
the poster. A post that silently went nowhere leaves both at zero and fails. It
also asserts the sink is installed first, so "turnloop carried this" cannot pass
with nothing listening.

**The edge does not move, and that is the honest answer.** `perry-ext-ioredis`
still declares `redis` and `tokio`, and the inventory still reads 29. Two
decline reasons survive, neither specific to redis and neither a hole in this
binding: the `tokio-wait-driver` A/B arm compiles no agent loop at all — it is
the baseline of the tokio-vs-turnloop measurement and exists to run the very
transport this replaces — and a host where `Loop::new` failed has no loop to
post to. Both are one shared decision taken once, not four conversions: every
plan-A and plan-B edge in `scripts/tokio_inventory.json` names the same pair.
The entries are updated to say so.
