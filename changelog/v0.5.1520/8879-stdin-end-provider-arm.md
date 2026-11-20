Restored the `end`/`close` arm on the stdin listener **provider** path, which
was lost when #8861 was batch-landed. Without it `echo hi | claude -p "…"`
never completes.

#8861 fixed piped stdin for `-p` by teaching three layers about `end`
listeners. Two of them landed: the GC rooting of `STDIN_END_CALLBACKS`, and
`STDIN_PULL_MODE`. The third — the `"end" | "close"` arm in `stdin_on_op` —
did not, and it is the one that made the fix work.

`stdin_on_op` is the provider that the stdin object's native
`on` / `once` / `addListener` methods delegate to: every registration that is
**not** codegen's literal `process.stdin.x(…)` shape. That means an alias
(`const s = process.stdin; s.once("end", …)`) or stdin passed as a parameter
(`helper(process.stdin)`). Claude Code's print-mode reader is exactly the
parameter form — `X71(process.stdin, 3000)`, then `stream.once("end", …)` on
the parameter — so those registrations fell into `_ => return` and were
silently discarded. The `end` half of the reader's
`race(once("end"), timeout(3000))` could never win.

`stdin_off_op` gets the mirror arm. #8864 removed the `end`/`close` clause from
the removal path, so a provider-registered listener could be added but never
removed — `removeListener` / `off` leaked it and the pump kept a stale callback
pointer.

The two tests that guarded this — `provider_path_registers_end_listeners` and
`provider_path_removes_end_listeners` — were dropped alongside the arm, which is
why CI stayed green through the regression. Both are restored here. They assert
the provider entry points (`stdin_on_op` / `stdin_off_op`) **directly** rather
than going through the `js_readline_stdin_on` extern, because the extern path
kept working the whole time and therefore cannot catch this. Sabotage-checked:
deleting the arm again fails `provider_path_registers_end_listeners`.

Measured on the claude-code 2.1.112 bundle, `echo hello | cc -p "…"`:

| build | result |
|---|---|
| before this fix | 4/4 hang (120 s timeout, no output) |
| with the arm restored | completes in ~7 s |
