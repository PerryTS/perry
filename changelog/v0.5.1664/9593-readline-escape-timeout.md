**fix(stdlib): clock bare Escape with readline's 500 ms timeout (#9593)**

A raw-mode Escape keypress no longer flushes on an arbitrary event-loop turn.
The readline pump now holds a torn escape prefix until an explicit 500 ms
deadline, matching Node's default `escapeCodeTimeout`. Bytes that finish an
arrow or other ANSI sequence cancel the one-shot, while expiry delivers a bare
Escape.

The deadline participates in the event pump's wait budget. Bare Escape therefore
arrives at the same time whether the loop is otherwise idle or an unrelated
timer is firing, and arrows split across reads remain a single keypress.
