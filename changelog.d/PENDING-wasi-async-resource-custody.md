Generated async C import adapters now distinguish argument consumption from
subtask completion and result initialization from JS ownership handoff. After
native cancellation acknowledgement, they drop committed owned arguments which
were never consumed and owned results which returned before cancellation but
were never lifted. Cleanup traverses records, tuples, lists, variants, options,
results and aliases, preserves borrowed handles, and precedes buffer and rooted
frame retirement. Normal inline and resumed completion keep ownership with JS. A failed owned
result lift traps before Promise settlement, including inline completion,
because a partial lift cannot safely reclaim the entire result.

A sanitizer-backed native C test executes the generated scheduler and cleanup
with deterministic terminal states, including queued start notifications,
return/cancel races, inactive payloads, failed import setup, and the inline frame
handoff boundary. This is an ownership prerequisite: experimental resource
profiles and public async guest generation remain guarded. Actual resource
component interop, exported result commits, borrowed export expiration, and
resource finalizer obligations during cancellation still require qualification.
