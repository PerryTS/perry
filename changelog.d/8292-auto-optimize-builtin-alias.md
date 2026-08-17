Auto-optimized builds now retain builtin constructor dispatch independently of
the optional fetch globals. Constructing collections and other builtins through
aliases or `globalThis` therefore matches the full prebuilt runtime, while the
`Headers` constructor remains correctly gated behind `global-webfetch`.
