The opt-in CLI helpers — the project-local .env loader, the RUST_LOG logger,
the terminal progress bar, the UTC formatter and the Apple JWT signer — move
into the `perry` binary crate as the internal `cli` module, which was their
only consumer. The `perry-cli-support` workspace member, its feature plumbing
(`logger`, `progress`, `dotenv`, `utc`, `apple-jwt`) and its manifest entry go
away; `p256` survives only behind `perry`'s `apple-api` feature, and the .env
parser-compatibility oracle (`dotenvy`) remains a dev-only dependency.
