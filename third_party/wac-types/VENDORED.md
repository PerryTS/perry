# wac-types 0.12.0

Pinned crates.io archive SHA-256 `5580726576266490359b1457abbc0393e1780295e48a6892500f2b7e68edc74d`. Upstream Cargo.lock is omitted in favor of Perry's workspace lock. The package version, license and upstream provenance are retained. Only src/aggregator.rs is patched.

Type aggregation must preserve the concrete interfaces of labeled imports before resolving shared dependencies. Canonical interface names can describe more than one imported instance, each with different nominal resources. The encoder primes these identities before aggregation; other canonical imports retain their existing semver merge behavior. Perry's composition tests require exact public import sets, correct resource ownership and rejection of the other labeled owner type.
