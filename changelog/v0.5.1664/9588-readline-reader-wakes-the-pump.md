**fix(stdlib): the stdin reader wakes the event loop after queueing input (#9588)**

Keystrokes reached the program only when the event loop happened to wake for
some other reason. `perry-stdlib`'s readline reader thread pushed what it read
into the shared `PENDING_DATA` / `PENDING_LINES` queues — which only the main
thread drains, from `js_readline_process_pending` — and went straight back into
`read(2)` without calling `js_notify_main_thread()`. The main loop was never
told the input existed, so delivery waited out whatever `js_wait_for_event` had
sized its sleep to: the next timer deadline, or, for a program sitting idle
waiting for input with no timer armed, the full one-second idle cap.

Measured on the claude-code bundle before the fix: 695-963 ms from keypress to
the `'data'` handler, against Node's sub-millisecond. After: 2.6 ms.

The reader now notifies once per `read(2)` that queued something, and once on
EOF so the `'end'`/`'close'` dispatch and the liveness flip are not delayed
either. This is the protocol the event pump documents and the one every other
cross-thread producer in the tree already followed — the child-process reactor,
the pty reactor, dgram, signals, and perry-runtime's own stdin reader in
`os_process_streams`. readline's reader was the only one that skipped it.
