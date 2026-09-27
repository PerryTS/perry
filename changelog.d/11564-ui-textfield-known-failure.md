**Parity allowlist: `test_issue_10155_textfield_singleline` joins the linux gtk4 family (#11560).**
The full tier's ~30 perry/ui "compile error" failures are all
`libperry_ui_gtk4.a not found` on the Linux parity host, which builds no
perry-ui-gtk4 archive; the family has been allowlisted as `ci-env` since #8271.
This fixture was added on 2026-09-13 (#10155) without an entry, so it was the
one perry/ui test the ratchet counted as a NEW failure in every full run since.

Also deletes five linux entries the full tier proves stale
(`test_guarded_raw_numeric_arrays`, `test_issue_3558_computed_accessors`,
`test_node_http2_basic`, `test_node_https_basic` — PASS in every full run
09-22..09-27 — and `test_issue_3199_3200_tls_server_tlssocket`, passing since
09-26). The ratchet fails a shard on a stale entry, and these alone kept parity
shards 11 and 12 red.
