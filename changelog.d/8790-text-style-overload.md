Fixed the `Text(content, style)` overload in `perry/ui`. Two-argument `Text`
calls were unconditionally routed through the reactive `Text(content, id)`
constructor, so a style object was passed to native UI backends as a string
pointer. Styled text examples consequently crashed on macOS while decoding the
bogus id. Codegen now distinguishes option objects from string ids, sends
styled text through the regular widget constructor plus inline-style setters,
and preserves the reactive-id route for `Text(content, id)`.

Doc-test failures now also retain the Unix signal number instead of reporting
every signal termination as the indistinguishable `exit=-1`, so release-gate
reports identify crashes and resource kills directly.
