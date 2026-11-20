### Fixed

- Fixed `super(...args)` when multiple evaluations of one class-expression
  template form an inheritance chain. The runtime now replays the exact pinned
  parent class object, including its capture snapshot and heritage, instead of
  reducing it to the template-wide class id. This prevents redis's shared
  Commander template from recursing or running the `RedisClientMultiCommand`
  constructor on a `RedisClient` instance (#10660).
