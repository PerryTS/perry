### A posted job that outlives its agent's loop is dropped, not run — documented

turnloop's `Loop` has no `Drop` that drains its postbox, so a host job still
queued when the owner's loop goes down is dropped without being invoked. The
context leaks rather than being freed twice, which is the safe direction, but a
caller whose job was going to settle a `JsPromise` gets neither a completion nor
an error.

It is bounded to agent teardown — `shutdown_agent_loop` for a retiring Worker,
or the process-exit funnel — when that agent's heap is going away regardless.
Named on `turnloop_post::post` and `perry_ffi::agent_post::post_job` rather than
left implicit, because "accepted" otherwise reads as "will run" with no
qualification, and a binding that must settle something can do it in its job's
`Drop`.
