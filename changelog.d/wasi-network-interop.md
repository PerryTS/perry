Accept omitted WASI provider imports when componentization has pruned unused
methods or whole interfaces. Present functions/resources still undergo type
and identity checks, a different interface version is rejected, and application
imports and all exports retain their strict contracts. This applies only when
WASI providers are explicitly enabled.

Add a Rust-authored WASI 0.2.12 networking component and native Perry host
acceptance coverage for TCP and connected/unconnected UDP traffic, exact protocol and
address/port grants, separate DNS permission and instance isolation. An
independent local peer verifies six authorized UDP exchanges and three TCP
exchanges and rejects
extra or missing traffic. No external DNS service is required.

This qualifies the component ABI and host providers, not `node:dgram` inside
Perry-authored WASI guests, cancellation, HTTP or P3. Execution qualification
is pending.
