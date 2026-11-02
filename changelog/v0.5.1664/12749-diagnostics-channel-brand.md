**Fix diagnostics-channel `instanceof` brand checks (#9202).**

Check the existing native brands for `Channel` and `BoundedChannel` before the
ordinary-object prototype shortcut, while preserving explicit own
`Symbol.hasInstance` overrides. Add a focused node-parity regression fixture for
dynamic constructors, noninstances, hook precedence, and descriptor restoration.
