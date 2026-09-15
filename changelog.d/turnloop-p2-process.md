### turnloop P2 — processes, pipes, datagrams and signals on the event loop

Continues the migration `docs/turnloop/p0-report.md` and `p1-report.md`
describe. P0 gave turnloop the *wait*; P1 gave it `node:net`'s sockets; P2
starts on what the design calls "many ad-hoc threads" — every thread whose only
job was to turn a blocking syscall into a queue push plus a
`js_notify_main_thread()`. Full writeup, including what did **not** move and the
specific turnloop API each of those is waiting on: `docs/turnloop/p2-report.md`.

**Moved off threads.**

- **`node:dgram`** — a bound socket used to get a thread that blocked in
  `recv_from` with a 250 ms read timeout, the timeout existing only so it could
  poll a `closing` flag; `close()` then had to unblock its own reader by sending
  the socket an empty datagram before joining the thread. Receives and sends are
  now operations on the agent's loop.
- **A child's stdout, stderr and extra `stdio` pipes** — one reader thread each,
  so two per child with piped stdio plus one per extra descriptor. Now one
  multishot read each.
- **OS signals** — `process.on('SIGINT', …)` started a process-wide
  `perry-signal-wake` thread that blocked in `read(2)` on a self-pipe the signal
  handler wrote one byte to. SIGINT, SIGTERM, SIGHUP, SIGUSR1 and SIGUSR2 are now
  loop subscriptions. SIGQUIT, SIGABRT, SIGBUS and SIGPIPE have no portable
  turnloop name (and ABRT/BUS are co-owned by the GC quarantine reporter), so
  they keep `sigaction` — and the wake thread now starts **only** if one of those
  four is subscribed, so a program handling SIGINT and SIGTERM starts none.

Everything downstream is unchanged by construction: the migrated producers push
the *same* events onto the *same* queues, drained by the *same* pumps, so the
tick a `'message'` or a `'data'` fires on, the AsyncLocalStorage context it
restores, and per-resource ordering are what they were. The thread paths survive
for an agent with no loop — a `worker_threads` agent before P3/P4, the
`tokio-wait-driver` A/B arm, a host where loop creation failed — which is the
coexistence rule P1 established.

**Perry creates the descriptors; turnloop only waits on them.** A dgram socket
carries Node's bind-time `SO_REUSEADDR`/`SO_REUSEPORT`/`IPV6_V6ONLY` choices and
its multicast state; a child's pipes come out of a `Command` with `pre_exec`
hooks turnloop's `ProcessSpec` cannot express. So P2 *adopts* the existing
descriptor rather than re-deriving syscalls that already work.

dgram keeps a `dup` for `setsockopt`/`getsockname`, because turnloop does not
expose the descriptor it owns. That sharing is the point — an option set through
the retained copy is the same `setsockopt` — and it is also why **sends** had to
move as well as receives: `O_NONBLOCK` lives on the shared open file
description, so a synchronous `send_to` through the retained copy would have
begun failing with `EWOULDBLOCK` the moment the socket buffer filled, where it
used to block.

**GC.** No JS heap memory reaches the driver: reads are copied out of turnloop's
pooled lease inside the dispatch call, on the owning thread; sends hand over an
owned `Vec<u8>`. New is one rooted value with a completion-scoped lifetime — a
`socket.send(msg, cb)` callback, which used to only have to survive a microtask
and now waits for a completion. It is held in the dgram registry and visited by
that module's already-registered root scanner, released exactly once at the
completion (or at a refused submission).

**One general consequence, worth knowing before the next phase.** A subsystem's
pump used to be self-sufficient — a thread had already pushed the bytes, so
draining the queue was the whole job. With a completion-shaped transport the
bytes exist only once the loop has been turned, so a caller that drives a pump
without parking spins against a queue nothing can fill. Both migrated pumps now
take one nonblocking turn first; it costs a thread-local read and no syscall
when the thread has adopted no descriptor.

**Not in this phase, with reasons in the report**: the child spawn and its exit
wait (`ProcessSpec.stdio` is three descriptors, and `fork()` needs the IPC
channel at fd 3 with `NODE_CHANNEL_FD`); the child IPC reader and the stdin
drain (both share an open file description with a write path that would become
non-blocking); pty; and `process.stdin`. `process.stdout`/`stderr` never had a
thread — they are synchronous writes with a `nextTick` callback — so there was
nothing to move.
