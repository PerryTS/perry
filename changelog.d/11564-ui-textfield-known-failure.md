**Parity allowlist: `test_issue_10155_textfield_singleline` joins the linux gtk4 family (#11560).**
The full tier's ~30 perry/ui "compile error" failures are all
`libperry_ui_gtk4.a not found` on the Linux parity host, which builds no
perry-ui-gtk4 archive; the family has been allowlisted as `ci-env` since #8271.
This fixture was added on 2026-09-13 (#10155) without an entry, so it was the
one perry/ui test the ratchet counted as a NEW failure in every full run since.
