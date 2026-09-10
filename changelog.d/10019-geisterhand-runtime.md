Fix Geisterhand startup crashes caused by cached runtime and stdlib archives
containing incompatible Rust runtime instances. Before linking, rebuild the full
Geisterhand library set in one Cargo invocation and use its reported artifact
paths throughout the link. Cargo still reuses fresh artifacts, but unrelated
archives in other build directories can no longer be mixed into the set.

Add a regression test that starts with two runtime variants and verifies that
preparation restores shared runtime state, plus coverage for fresh artifacts,
incomplete output, and failed builds. Geisterhand builds require a Perry source
checkout, discoverable through `PERRY_WORKSPACE_ROOT` when needed. Fixes #10019.
