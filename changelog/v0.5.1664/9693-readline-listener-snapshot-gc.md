Fixed intermittent terminal-input hangs in long-running Claude Code sessions.
`process.stdin.listeners()` now roots its callback snapshot before allocating
the returned array, so moving GC cannot leave Ink's stdin suspend/resume path
with stale `readable` callbacks. Permission dialogs therefore continue to
receive input after GC pressure.

A forced-evacuation regression test pins the snapshot/allocation boundary that
previously exposed the stale pointer, and an Ink-style PTY stress fixture
confirmed input delivery after repeated suspend/remove/restore cycles.
