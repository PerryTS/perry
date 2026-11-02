**turnloop: hand work to the loop of an agent another thread owns (perry#10395 step 1).**

19 of the tokio manifest edges left in this PR are not nineteen problems — they
are one. Every one of them is the *decline path*: a thread that cannot get a
turnloop loop of its own keeps its tokio implementation as a live fallback.
Since P9 gave every JS agent its own loop, that declining thread is no longer
"a worker"; it is a second thread acting **for** an agent another thread owns —
a host pump thread, Android's UI thread for `perry-native`.

turnloop already ships the missing half and Perry did not use it. A `Route` now
carries the loop's `Poster` alongside its `Notifier`, published and cleared at
the same two places, and `event_pump::post_to_agent(agent, token, payload)`
hands work to that agent's owner from any thread.

Posting is sound precisely because both threads serve the **same agent's heap**:
the completion is delivered on the owner, which is where that agent's JS values
live. Nothing crosses an agent boundary — this is not the rejected "route
completions between agents" idea.

Two things it deliberately does *not* do. It never silently declines:
`PostToAgentError` separates `NoRoute` (no loop exists — use your own fallback)
from `NotPublished` (transient: the owner is mid-build) from `Refused` (that
loop exists and said no). And it does not flatten `Poster::post`'s retry
contract: a refused post hands the payload **back** so the caller can retry,
while a wake error *after* enqueue returns `None`, because the post was accepted
and retrying it would deliver twice.

The poster is cloned out from under the `ROUTES` lock before posting, so a
cross-thread wake never happens inside a mutex every producer takes.

Two tests assert the mechanism rather than the absence of a panic: one has a
foreign thread post to an agent's owner and checks the payload arrives on the
owner exactly once with its token and value intact (and that an agent with no
route gets `NoRoute`, not silence); the other fills the bounded postbox and
checks the refused payload comes back, then drains it and counts every accepted
post delivered exactly once.

Step 2 — converting the declining bindings one at a time, starting with
`perry-ext-net`'s socket task — is what actually removes the edges; this is the
enabler they were all waiting on, which is why it is `pub`.
