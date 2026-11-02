### Fixed

The parity aggregate CI job now skips when an upstream failure prevented the
parity shards from running, while still collecting reports from shards that ran
and failed.
