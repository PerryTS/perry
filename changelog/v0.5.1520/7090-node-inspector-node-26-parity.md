Completed Node.js 26.5 inspector parity across callback and Promise sessions,
including protocol result shapes, lifecycle validation, notifications, endpoint
state, network helpers, and console integration. Inspector console calls now
emit their real method-specific event types, listener removal honors an optional
event name, and every outstanding Promise post remains rooted and is rejected
on disconnect.
