### turnloop P8 — the tokio inventory becomes a gate, and says what is left

The turnloop migration's remaining surface was recorded only as prose: eight
lane reports, each ending with a list of what it did not move, each written at a
different commit. Read against the tree, those lists are incomplete (none names
the `perry` CLI, `perry-container-compose` or `perry-ui-gtk4`, which hold 6 of
the 46 remaining edges between them), scope the same blocker differently each
time, and contain no number at all — so nobody could say how much was left, or
when it would be done.

**`scripts/tokio_inventory.py`** re-derives it from the tree and runs in the
required `lint` job. It gates two exact, machine-derived facts: every
(workspace crate → tokio-family crate) manifest edge, and every tokio-family
package in `Cargo.lock`, compared strictly against
`scripts/tokio_inventory.json` **in both directions**. A new edge fails, so
tokio cannot creep back in behind a green build; a *stale* entry fails too, so a
lane that removes an edge must delete its own line and the file can never
describe a tree that is gone. Each entry carries what `cargo tree` cannot: what
JS reaches the edge, when a program takes it, what blocks its removal, and where
that is tracked. `--table` renders them.

Edges come from `cargo metadata --no-deps` rather than `cargo tree`, because
`cargo tree -i tokio --workspace` is wrong in both directions on this tree: it
names `perry-runtime`, `perry-runtime-static`, `perry-updater`,
`perry-ext-node-forge` and `perry-ext-undici` as tokio dependents (none has an
edge — the hit is feature unification through `timezone_provider → combine`),
and it cannot see `perry-ui-gtk4`'s `cfg(target_os = "linux")` tokio at all
from a macOS host.

The state it records: **46 manifest edges across 16 workspace crates, 20
tokio-family packages in `Cargo.lock`** — unchanged by this branch. The full
costed removal plan is in `docs/turnloop/p8-report.md`. Its headline is that one
missing capability, a `turnloop::Loop` per agent, is what keeps a tokio socket,
a `reqwest::Client`, a `hyper` server, `sqlx`, the `redis` crate, the `mongodb`
driver and lettre's async transport all reachable from ordinary JavaScript.

Two smaller changes ride along. `perry-stdlib`'s `js_cron_set_interval` and
`js_cron_set_timeout` no longer spawn a native task: that task's whole body was
the `// Invoke callback (in real impl: …)` placeholder `cron.rs`'s own header
records as the bug `js_cron_schedule` was rewritten to fix, so it could not do
the thing it existed for, and a cleared 24-hour interval held the task for a
day. Nothing lowers to those symbols and `perry-ext-cron` — the copy the
well-known flip actually links — has always been a bare handle allocator, so the
two copies now agree. And `scripts/turnloop/apps/tokio_worker_agent_census.ts`
measures the decline every lane depends on instead of asserting it.

Building the probe for that last point turned up a **regression on
`turnloop/integration`**: `fetch()` inside a `node:worker_threads` Worker
answers 200 on `main` and `error: fetch failed` on the branch, because a
`worker_threads` Worker never claims an agent id and so reports itself as the
primary agent to every turnloop availability check. That, three further Perry
defects and a costed removal plan whose fourteen groups account for all 46
edges are in `docs/turnloop/p8-report.md`.
