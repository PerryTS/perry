Disposing a component now runs the destructor of every owned guest resource
that is still live, exactly once, in the disposal frame. Since all non-HTTP
components moved to the persistent driver, disposal dropped the driver and
discarded its Store, so a live owner (for example an abandoned constructor
result) was never destroyed. Disposal now closes the driver's submission
channel: queued commands are refused without running guest code, in-flight
commands are cancelled, and the Store returns to `Host::dispose`, which drains
live owners in the same order as a legacy handle. This holds while a guest call
is still suspended. A driver that has already retired its Store (after a
cancelled started call or a failing destructor) has no owners left.

Driver tests cover a quiescent component and one with a suspended export and a
queued call; both were red on main. The `resources-run` native component
acceptance step is green again.
