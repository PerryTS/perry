Remove 22 unused Rust dependency declarations and the unused `similar` workspace
template, including stdlib's unused Clap and Tokio cron scheduler dependencies.
The lockfile drops 15 external package versions without upgrading or adding any
packages. Existing Tokio usage and public APIs are unchanged.

Add an assessment of all 195 baseline direct Rust dependencies, reproducible
dependency inventory tooling, and recommendations for feature trimming and
selective implementation ownership. Graph measurements are distinguished from
unmeasured binary-size and runtime-performance effects.
