Bumped `rustls` 0.23.44 → 0.23.45, clearing **RUSTSEC-2026-0285** (#10791).

TLS 1.3 handshake messages were accepted across encryption-level boundaries
(medium, 5.3). `cargo audit` has failed on it since the advisory was published;
it is the one unignored finding in the workspace, and `security-audit` runs on
every PR that touches a lockfile — so every merge train hit it.

A single-crate lock bump, no manifest change and no cascade ("Locking 1 package
to highest compatible version"). 0.23.45 cleared the repo's 7-day
`SOAK_DAYS` window on 2026-09-21, which is what #10791 was waiting for.
