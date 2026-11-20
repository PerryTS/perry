The `compile-smoke` CI job builds the database and mailer extension wrappers in
the same cargo invocation as the stdlib archive. Building them separately let
cargo unify tokio differently for each, and the linker guard correctly refused
the resulting pair — two tokio compilations in one binary mean two independent
runtime contexts and a "no reactor running" panic.
