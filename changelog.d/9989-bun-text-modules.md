### Fixed

- Embedded Bun text-loader modules can now be required as strings with stable cache identity, empty-file support and module-relative resolution. File-loader assets remain byte assets; invalid extraction metadata is rejected.
- Build the standalone native regression's runtime and require-provider archives in one Cargo graph, preserving the Tokio coherence check on fresh CI runners.
