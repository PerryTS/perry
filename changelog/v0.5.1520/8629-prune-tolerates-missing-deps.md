Fixed the full-tier `cargo-test` job failing after every test passed. The step
prunes linked test binaries between packages with a bare
`find target/debug/deps … -delete`, but that directory only exists once
something has built into it — when the job's scope builds straight to
`--release` and no `cargo test -p <package>` has run yet, `find` exits 1 and
GitHub's default `bash -e` fails the whole job.

Both prune calls now tolerate a missing directory. Note the guard also
suppresses genuine `find` errors: the prune is disk hygiene, so its failure
should never fail the job, but if it ever silently stops working the symptom
would be a later disk exhaustion rather than a pointed error.
