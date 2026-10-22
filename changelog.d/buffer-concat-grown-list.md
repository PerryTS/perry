`Buffer.concat(list)` reads a list array that has grown past its birth
storage. An array returned as `[chunk]` from a function is allocated at exact
capacity; its first `push` moves the elements and leaves a forwarding stub, and
`Buffer.concat` read the stub's slots, so it threw `The "list[1]" argument must
be an instance of Buffer or Uint8Array` (or crashed with SIGBUS on garbage).
effect's NodeStream pull builds exactly such lists, so every opencode git
command with more than one output chunk failed and the session loop could not
start a turn.
