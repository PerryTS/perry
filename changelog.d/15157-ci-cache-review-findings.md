Rust cache keys in CI carry the ISO week, so an entry saved by a job that
failed before building lasts at most a week. Jobs whose only cache saver ran
on pull requests now own families that `main` saves, and a test runs the
vendored rust-cache cleanup to check that it keeps the build-std crates.
