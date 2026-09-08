### Fixed

- Materialize lazy Error stacks when `util.inspect()` formats an Error with a
  `cause` or an `AggregateError` with `errors`, preserving Node-compatible
  stack/body layout.
