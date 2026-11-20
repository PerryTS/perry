fix(async): an `await` inside a `finally` of an `async function*` no longer
compiles to a blocking busy-wait. When a `try` in an async generator has a
finally that yields or awaits, the finally is linearized into its own dispatch
states, and `.next()`/`.throw()` drive those states through the shared async-step
driver so their `await`s suspend on the microtask queue. The `.return()` closure,
however, re-drove the same states through a separate busy-wait dispatch loop
(`__sent = await value; continue`) — so a `.return()` that ran the finally (an
early `break` in a `for await`, or an explicit `.return()`) block-waited on the
finally's `await`, monopolising the single runtime thread and deadlocking. This
is the finally analog of the #8681 `await`-in-`catch` deadlock.

`.return()` now hands the continuation off to the shared `__agstep` driver
(a fresh non-error resume) after routing the pending return into the finally,
exactly as `.next()`/`.throw()` already do, so a finally `await` suspends
instead of blocking. Behavior for a finally that only yields is unchanged.
