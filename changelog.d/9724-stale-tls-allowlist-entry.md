**Drop the stale `readline_helpers.rs` entry from the cold-`thread_local!`
allowlist.** #9697 removed that file's only raw `thread_local!` block but left
its justification behind, and `scripts/check_thread_locals.py` fails a recorded
entry that no longer matches the tree — "a stale entry is one nobody has to
justify". The check runs in `tls-budget.yml`, a satellite workflow outside the
64-gate `lint` set, so it was not caught at merge time.
