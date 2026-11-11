`perry-codegen` no longer declares a dependency on `perry-runtime-manifest`; it
never referenced it, so the edge only constrained build order. The crate policy
also stops disagreeing with the code it describes: `perry-ui-windows-winui` is
recorded `keep` instead of `merge` (it owns an independently selected
`--target windows-winui` artifact over a vendored dependency graph, which is the
policy's own `keep` test), and `perry-codegen-{swiftui,glance,wear-tiles}` are
recorded `keep` instead of `review` — `review` means missing usage evidence, and
all three are wired, documented and tested; the pending decision was about
consolidating their shared widget walk, which is a separate change.
