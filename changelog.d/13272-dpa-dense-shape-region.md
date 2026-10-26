Object layout words now come from a dense per-agent shape Region. A layout word
is a static prefix ordinal (records the compiler or runtime publishes once,
shared by every agent, whose pages are committed once at agent creation) plus
per-agent dynamic rows (published into the agent own Region, one page per row,
so a record is read with a single bound compare). Chunks are never reused, so
a stale word can never name a different record, and exhausting the row space is
fatal rather than silently wrapping. Function constructors keep their instance
record on the prototype holder (validated per use), so new F() stays O(1).
Startup instruction counts are unchanged to slightly lower than main.

Computed string reads continue to call inherited getters after deleting an own property.
