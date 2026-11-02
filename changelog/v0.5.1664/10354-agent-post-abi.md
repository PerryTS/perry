### turnloop P10: a binding can run a job on its agent's loop, instead of keeping tokio

`event_pump::post_to_agent` (perry#10395 step 1) let any thread hand work to the
loop of an agent another thread owns, but no binding crate could reach it: the
four database extensions depend on `perry-ffi` only, and `post_to_agent` had no
C-ABI export. This adds the seam.

**The decline it targets.** Every remaining tokio edge in a binding is held by
one sentence — *"a thread that could not get a loop of its own"*. Since turnloop
P9 gave every JS agent a loop, that thread is not a worker; it is a **second
thread acting for an agent another thread already owns**. Android is the shape:
`perry-native` runs the compiled TypeScript on the primary heap while the UI
thread pumps for the same agent, and whichever claims the route first leaves the
other unable to submit. Its only answer was a whole async runtime of its own.

**The ABI.** Three `#[no_mangle]` functions in the new
`perry-runtime/src/turnloop_post/`, and one rule:

| symbol | answers |
|---|---|
| `js_perry_agent_post_available()` | would a post from this thread reach a loop? Asked once, at connection creation, because posting *consumes* its context and "would this land?" cannot be answered by trying. |
| `js_perry_agent_post(run, ctx)` | run `run(ctx)` on the agent's owner. |
| `js_perry_agent_post_dispatched()` | posted jobs **this thread has run** — the liveness counter. |

The rule is the sign of the return code: **`>= 0` means the runtime owns `ctx`
and will invoke the callback exactly once; `< 0` means `ctx` is untouched and
still the caller's.** `Poster::post`'s retry contract survives intact through
that split — a wake failure after enqueue is `1`, on the *consumed* side,
because the job is queued and retrying would deliver it twice; `-2` (no loop at
all, use your fallback) and `-3` (transient) are the two that hand it back.

Three design choices worth naming:

- **The ABI takes no agent argument.** The runtime resolves
  `agent::current_agent()` itself. `post_to_agent` is sound precisely because
  both threads serve the same agent's heap; an ABI that let a binding name an
  arbitrary agent would make the unsound call expressible. Now it is not.
- **Its own token space, 0x30..=0x3F.** The router in `dispatch_staged` is a
  sequence of range tests with P1 as the *fall-through*, so a new class that is
  not branched on explicitly does not fail loudly — it hands a boxed job to the
  net sink, which reads the token's low bits as a socket id. A test asserts the
  band is disjoint from P2's, P4's and P3's timer, and that P1's own 1..=7 do
  not read as posted jobs.
- **No layout digest.** `perry-ffi::turnloop_net` needs one because both sides
  declare their own copy of `NetCompletion`; this ABI shares no struct — a
  function pointer, a `void*` and an `i32` — so there is nothing to drift and a
  digest would only be a second thing to keep in step.

**The Rust face**, `perry_ffi::agent_post`, makes the ownership rule
un-get-wrong-able: `post_job<J: AgentJob>(Box<J>) -> Result<(), Rejected<J>>`
takes the box and gives it back only when the post did not land, with
`Rejected::NoRoute` (use your fallback) kept apart from `Rejected::Again`
(retry). The trampoline is monomorphised per job type, so there is no
type-erasure step and no second allocation.

**Tests assert the subject ran.** `a_posted_host_job_runs_on_the_owner_not_on_
the_poster` checks the callback fired **on the owner thread and not the
poster's** — that is the whole point, because the owner is where the agent's JS
values live — exactly once, with its context intact, and watches
`turnloop_post::dispatched()` move so a green run cannot mean "nothing
listened". The refusal path asserts the reclaim: a `Drop`-counting context is
dropped exactly once, by the caller, after a refused post. The
`tokio-wait-driver` A/B arm has its own test asserting it declines, since it
compiles no agent loop and is the one decline reason posting cannot close.

**Also:** an isolated `cargo check -p perry-ffi --all-targets` was red before
this, and had been for a while — `turnloop_net`'s extern declarations, `check`,
`endpoint`, `REGISTERED` and the `OK`/`ENOLOOP` codes are all unreachable in a
build with no runtime linked. A workspace build hides it, because a binding
crate's `perry-ffi = { features = ["runtime-link"] }` dev-dependency unifies the
feature on. Gated, so the isolated check is green and the next dead item in that
seam is visible instead of buried in nine that were already there.
