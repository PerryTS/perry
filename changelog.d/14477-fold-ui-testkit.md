The UI test-mode plumbing (test-mode detection, exit delay, and screenshot
path handling) moves from the separate `perry-ui-testkit` crate into
`perry-ui` itself, which every UI backend already depends on. This removes a
workspace member and seven manifest edges with no change in behaviour.
