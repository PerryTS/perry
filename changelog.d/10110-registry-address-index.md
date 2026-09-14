Prepare the address-index fix for #10110: allow each registry to size its Bloom
filter and add `RegistryAddrIndex`, which rebuilds its bits from live addresses
as registrations retire. Readers retain three atomic bit tests with no lock or
allocation. Existing symbol filters retain their 16-word width and hash positions.

Adapted from [proggeramlug/perry#1](https://github.com/proggeramlug/perry/pull/1).
Coverage includes the reported 644-address population, retirement, 20,000-address
churn, address reuse, and concurrent admissions/refreshes. Hash slices scale with
the chosen width so all three probes can use the entire array.

The canonical-wrapper subsystem is still absent from `main`, so this is its
prerequisite, not a claim of the campaign's measured CPU improvement on `main`.
When that subsystem lands, use `RegistryAddrIndex<256>` in
`native_handle/canonical.rs`, admit before publishing the identity, and retire
after all three removal paths: stale identity replacement, finalized identity
removal, and explicit retirement (after `finalize_once`). Keep the authoritative
wrapper validation after `may_contain`. Issue #10110 remains open for that wiring.
