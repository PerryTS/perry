Stop the `perry-ext-http` server activity-count tests from racing each other
on the primary agent's shared count.

The primary agent's active-server count is one `static` shared by every
primary-agent thread, and every cargo test thread is one, so
`activity_count_does_not_include_another_agents_servers` compared it against a
baseline while other server tests created and dropped servers, and failed about
1 run in 10 at default parallelism. The count itself is correct and per-agent as
designed. The exact-count test now runs as a worker agent, whose count is
thread-local, and the cross-agent test asserts counter identity instead of the
shared absolute value. Test-only; no runtime code changes.
